//! Operational CLI for the `events` service.
//!
//! Wraps the same sports-client and persistence layers the long-running
//! ingester uses, but for one-shot tasks an operator runs by hand:
//!
//!   - `seed-tournament --competition WC` — fetches the competition's teams
//!     and group standings from football-data.org and upserts them into
//!     `teams`, `tournament_groups`, and `tournament_group_teams`.
//!   - `list-matches --competition WC` — read-only dump of every fixture
//!     for the competition (kickoff, stage, group, status, score). Useful
//!     for spot-checking the upstream feed before a write-shaped command.
//!   - `list-teams --competition WC` — read-only dump of every team
//!     participating in the competition.
//!
//! All commands honour `DATABASE_URL` and `FOOTBALL_API_KEY` from the
//! environment (loaded from `.env` by the `justfile`), so no secrets ever
//! travel through CLI flags.

use std::collections::HashMap;

use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use domain::{
    repository::{TeamRepository, TournamentGroupRepository},
    Team, TournamentGroup, TournamentGroupAssignment,
};
use events::ingester::football_data::{
    flag_emoji_for_team, group_assignments_from_matches, group_label_from_upstream,
    phase_from_stage, team_country_code,
};
use persistence::{
    init_pool,
    repositories::{teams::PgTeamRepository, tournament_groups::PgTournamentGroupRepository},
};
use rust_utils::secret::Secret;
use shared::Config;
use sports_client::{Client as SportsClient, MatchStatusDto, ScoreDto, ScoreDuration};
use tracing::{info, warn};
use uuid::Uuid;

/// Top-level CLI definition. `--competition` lives on each subcommand
/// (rather than at the root) so future subcommands that do not target a
/// single competition (e.g. a global cleanup task) compose cleanly.
#[derive(Debug, Parser)]
#[command(
    name = "events-cli",
    version,
    about = "Operational CLI for the events service: seed tournament data and inspect upstream feeds.",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Pull teams + group standings from football-data.org and upsert them
    /// into the local DB. Idempotent — re-running refreshes display fields
    /// without producing duplicates.
    SeedTournament {
        /// Competition code, e.g. `WC` (FIFA World Cup), `EC` (UEFA Euros),
        /// `PL` (English Premier League).
        #[arg(short, long, default_value = "WC")]
        competition: String,
        /// When set, perform every API call but skip every DB write. Useful
        /// to validate the upstream payload before touching the database.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print every match the upstream knows about for the competition.
    ListMatches {
        #[arg(short, long, default_value = "WC")]
        competition: String,
        /// Optional status filter (case-insensitive): scheduled, finished,
        /// in_play, postponed, ... Matches the upstream `status` enum.
        #[arg(long)]
        status: Option<String>,
    },
    /// Print every team the upstream lists for the competition.
    ListTeams {
        #[arg(short, long, default_value = "WC")]
        competition: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::tracing::init("events-cli")?;
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;
    let cli = Cli::parse();

    match cli.command {
        Command::SeedTournament {
            competition,
            dry_run,
        } => seed_tournament(&config, &competition, dry_run).await,
        Command::ListMatches {
            competition,
            status,
        } => list_matches(&config, &competition, status.as_deref()).await,
        Command::ListTeams { competition } => list_teams(&config, &competition).await,
    }
}

/// Build a sports-client from the shared config. Wraps the raw API key in a
/// `Secret` so accidental log lines never leak it.
fn build_client(config: &Config) -> anyhow::Result<SportsClient> {
    SportsClient::new(Secret::new(config.football_api_key.expose().to_string()))
        .map_err(shared::report_to_anyhow)
        .context("building football-data sports client")
}

/// Read-only dump of every match for a competition, optionally filtered by
/// upstream status string.
async fn list_matches(
    config: &Config,
    competition: &str,
    status_filter: Option<&str>,
) -> anyhow::Result<()> {
    let client = build_client(config)?;
    let payload = client
        .get_competition_matches(competition)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("fetching matches for {competition}"))?;

    let filter_upper = status_filter.map(|s| s.trim().to_ascii_uppercase());
    println!(
        "{:<22}  {:<14}  {:<10}  {:<10}  {:<32}  {:<32}  {:<10}  match_id",
        "kickoff_utc", "stage", "group", "status", "home", "away", "score"
    );
    println!("{}", "-".repeat(160));

    let mut shown = 0;
    for m in &payload.matches {
        if let Some(filter) = filter_upper.as_deref() {
            // Compare on the upstream string by serializing the typed enum;
            // simpler than dragging a string field along just for filtering.
            let upstream = match m.status {
                MatchStatusDto::Scheduled => "SCHEDULED",
                MatchStatusDto::Timed => "TIMED",
                MatchStatusDto::InPlay => "IN_PLAY",
                MatchStatusDto::Paused => "PAUSED",
                MatchStatusDto::Finished => "FINISHED",
                MatchStatusDto::Suspended => "SUSPENDED",
                MatchStatusDto::Postponed => "POSTPONED",
                MatchStatusDto::Cancelled => "CANCELLED",
                MatchStatusDto::Awarded => "AWARDED",
                MatchStatusDto::Unknown => "UNKNOWN",
            };
            if upstream != filter {
                continue;
            }
        }

        let phase = phase_from_stage(m.stage.as_deref());
        let group = group_label_from_upstream(m.group.as_deref()).unwrap_or_default();
        let stage = m.stage.as_deref().unwrap_or("-");
        let status = format!("{:?}", m.status).to_lowercase();
        let home = home_team_label(m);
        let away = away_team_label(m);
        let score = format_score(m.score.as_ref());

        println!(
            "{:<22}  {:<14}  {:<10}  {:<10}  {:<32}  {:<32}  {:<10}  {}  ({})",
            m.utc_date,
            stage,
            group,
            status,
            truncate(&home, 32),
            truncate(&away, 32),
            score,
            m.id,
            phase_short(phase),
        );
        shown += 1;
    }

    println!(
        "\n{} matches shown ({} total).",
        shown,
        payload.matches.len()
    );
    Ok(())
}

/// Read-only dump of every team for a competition.
async fn list_teams(config: &Config, competition: &str) -> anyhow::Result<()> {
    let client = build_client(config)?;
    let payload = client
        .get_competition_teams(competition)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("fetching teams for {competition}"))?;

    println!("{:<6}  {:<32}  {:<6}  {:<6}", "tla", "name", "code", "flag");
    println!("{}", "-".repeat(60));
    for t in &payload.teams {
        let code = team_country_code(t).unwrap_or_default();
        let flag = flag_emoji_for_team(t);
        println!(
            "{:<6}  {:<32}  {:<6}  {}",
            t.tla.clone().unwrap_or_default(),
            truncate(&t.name, 32),
            code,
            flag,
        );
    }
    println!("\n{} teams.", payload.teams.len());
    Ok(())
}

/// Seed `teams`, `tournament_groups`, and `tournament_group_teams` from the
/// upstream. Order matters:
///   1. Teams first (other tables FK to it).
///   2. Tournament groups (no FK on each other).
///   3. Assignments (FK both teams and tournament_groups).
///
/// The function is structured so a single failure mid-way leaves the DB in a
/// recoverable state: every step is idempotent, so a re-run picks up where
/// the previous attempt left off.
async fn seed_tournament(config: &Config, competition: &str, dry_run: bool) -> anyhow::Result<()> {
    let client = build_client(config)?;

    info!(competition, "fetching teams from upstream");
    let teams_payload = client
        .get_competition_teams(competition)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("fetching teams for {competition}"))?;

    // Group assignments come from /matches, not /standings: the standings
    // endpoint is empty before the competition starts (the case for the WC
    // 2026 right now), but every group-stage match in /matches already
    // carries a populated `group` field. We fall back to /standings only
    // when /matches yields no group-stage entries.
    info!(competition, "fetching matches from upstream");
    let matches_payload = client
        .get_competition_matches(competition)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("fetching matches for {competition}"))?;

    let mut assignments = group_assignments_from_matches(&matches_payload.matches);
    if assignments.is_empty() {
        info!("no group-stage matches yet; falling back to /standings");
        let standings = client
            .get_competition_standings(competition)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("fetching standings for {competition}"))?;
        assignments =
            events::ingester::football_data::group_assignments_from_standings(&standings.standings);
    }

    if dry_run {
        return print_seed_summary(&teams_payload.teams, &assignments);
    }

    let pool = init_pool(config.database_url.expose())
        .await
        .context("connecting to Postgres")?;
    let team_repo = PgTeamRepository::new(pool.clone());
    let group_repo = PgTournamentGroupRepository::new(pool.clone());

    // Step 1 — upsert every team so we can resolve country_code → UUID later.
    let mut teams_written = 0;
    let mut country_to_id: HashMap<String, Uuid> = HashMap::new();
    for dto in &teams_payload.teams {
        let Some(code) = team_country_code(dto) else {
            warn!(name = %dto.name, "team has no country code, skipping");
            continue;
        };
        // Re-use an existing UUID when present so any FK keeps pointing at
        // the same row across re-seeds.
        let existing = team_repo
            .find_by_country_code(&code)
            .await
            .map_err(shared::report_to_anyhow)?;
        let id = existing.as_ref().map(|t| t.id).unwrap_or_else(Uuid::new_v4);
        let team = Team {
            id,
            name: dto.name.clone(),
            flag_emoji: flag_emoji_for_team(dto),
            country_code: code.clone(),
        };
        team_repo
            .upsert(&team)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("upserting team {}", code))?;
        country_to_id.insert(code, id);
        teams_written += 1;
    }
    info!(count = teams_written, "teams upserted");

    // Step 2 — upsert every tournament group named in the assignments.
    let mut group_to_id: HashMap<String, Uuid> = HashMap::new();
    let group_names: Vec<String> = unique_preserve_order(assignments.iter().map(|(g, _)| g));
    for name in &group_names {
        let existing = group_repo
            .find_by_name(name)
            .await
            .map_err(shared::report_to_anyhow)?;
        let id = existing.as_ref().map(|g| g.id).unwrap_or_else(Uuid::new_v4);
        group_repo
            .upsert(&TournamentGroup {
                id,
                name: name.clone(),
            })
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("upserting group {name}"))?;
        group_to_id.insert(name.clone(), id);
    }
    info!(count = group_to_id.len(), "tournament groups upserted");

    // Step 3 — wire teams to their groups. We tolerate missing teams (a
    // standings row may reference a placeholder team that did not appear in
    // /teams) by logging instead of aborting.
    let mut assigned = 0;
    let mut skipped_missing_team = 0;
    for (group_name, country_code) in &assignments {
        let Some(team_id) = country_to_id.get(country_code) else {
            warn!(
                country_code,
                group_name, "team not in /teams payload, skipping"
            );
            skipped_missing_team += 1;
            continue;
        };
        let Some(group_id) = group_to_id.get(group_name) else {
            // Should be impossible since we built group_to_id from the same
            // source list, but defensively log instead of indexing-panic.
            warn!(group_name, "tournament group missing from local cache");
            continue;
        };
        group_repo
            .assign_team(&TournamentGroupAssignment {
                tournament_group_id: *group_id,
                team_id: *team_id,
            })
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("assigning {country_code} to {group_name}"))?;
        assigned += 1;
    }
    info!(
        assignments = assigned,
        missing = skipped_missing_team,
        "team-to-group assignments complete"
    );

    println!(
        "Seed complete: {} teams, {} groups, {} assignments ({} skipped due to missing teams).",
        teams_written,
        group_to_id.len(),
        assigned,
        skipped_missing_team
    );
    Ok(())
}

/// Pretty-print what `seed-tournament --dry-run` would do without touching
/// the database. Mirrors the structure of the real seed.
fn print_seed_summary(
    teams: &[sports_client::TeamDto],
    assignments: &[(String, String)],
) -> anyhow::Result<()> {
    println!("Teams ({}):", teams.len());
    for t in teams {
        let code = team_country_code(t).unwrap_or_default();
        println!(
            "  {:<6}  {:<32}  {:<6}",
            t.tla.clone().unwrap_or_default(),
            truncate(&t.name, 32),
            code,
        );
    }
    let unique_groups = unique_preserve_order(assignments.iter().map(|(g, _)| g));
    println!("\nGroups ({}):", unique_groups.len());
    for name in &unique_groups {
        let mut teams_in_group: Vec<&str> = assignments
            .iter()
            .filter(|(g, _)| g == name)
            .map(|(_, c)| c.as_str())
            .collect();
        teams_in_group.sort_unstable();
        teams_in_group.dedup();
        println!("  {}  ->  [{}]", name, teams_in_group.join(", "));
    }
    if assignments.is_empty() {
        return Err(anyhow!(
            "no group assignments found — is the competition code correct?"
        ));
    }
    Ok(())
}

/// Collect items in encounter order, dropping duplicates. Used to derive
/// the unique group list from a `(group, country)` stream while preserving
/// the upstream's natural ordering (Group A, Group B, ...).
fn unique_preserve_order<'a, I, S>(iter: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a S>,
    S: AsRef<str> + 'a + ?Sized,
{
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in iter {
        let s = item.as_ref();
        if seen.insert(s.to_string()) {
            out.push(s.to_string());
        }
    }
    out
}

fn home_team_label(m: &sports_client::MatchDto) -> String {
    m.home_team
        .name
        .clone()
        .or_else(|| m.home_team.tla.clone())
        .unwrap_or_else(|| "TBD".to_string())
}

fn away_team_label(m: &sports_client::MatchDto) -> String {
    m.away_team
        .name
        .clone()
        .or_else(|| m.away_team.tla.clone())
        .unwrap_or_else(|| "TBD".to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Render the score as a compact "h-a" or "h-a (a.e.t.)" / "h-a (pens X-Y)"
/// suffix when applicable, "-" when no score yet.
fn format_score(score: Option<&ScoreDto>) -> String {
    let Some(s) = score else {
        return "-".to_string();
    };
    let Some(ft) = s.full_time else {
        return "-".to_string();
    };
    let Some(home) = ft.home else {
        return "-".to_string();
    };
    let Some(away) = ft.away else {
        return "-".to_string();
    };
    let base = format!("{home}-{away}");
    match s.duration {
        Some(ScoreDuration::ExtraTime) => format!("{base} (a.e.t.)"),
        Some(ScoreDuration::PenaltyShootout) => match s.penalties {
            Some(ph) => format!(
                "{base} (pens {}-{})",
                ph.home.unwrap_or(0),
                ph.away.unwrap_or(0)
            ),
            None => format!("{base} (pens)"),
        },
        _ => base,
    }
}

fn phase_short(phase: domain::Phase) -> &'static str {
    match phase {
        domain::Phase::Group => "group",
        domain::Phase::Knockout => "ko",
    }
}
