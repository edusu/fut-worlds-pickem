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

use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use events::ingester::bootstrap::{
    ensure_teams_and_assignments_seeded, ensure_tournament, ensure_tournament_structure,
    V1_TOURNAMENT_EXTERNAL_ID,
};
use events::ingester::football_data::{
    flag_emoji_for_team, group_assignments_from_matches, group_label_from_upstream,
    phase_from_stage, team_country_code,
};
use persistence::{
    init_pool,
    repositories::{
        knockout_phases::PgKnockoutPhaseRepository, teams::PgTeamRepository,
        tournament_groups::PgTournamentGroupRepository, tournaments::PgTournamentRepository,
    },
};
use rust_utils::secret::Secret;
use shared::Config;
use sports_client::{Client as SportsClient, MatchStatusDto, ScoreDto, ScoreDuration};
use tracing::info;

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
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;
    let _tracing_guard = shared::tracing::init(
        "events-cli",
        config.otel_endpoint.as_deref(),
        config.otel_service_namespace.as_deref(),
    )?;
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
/// upstream. The actual writes are delegated to the bootstrap helpers the
/// ingester also uses, so the CLI and the long-running service share a
/// single source of truth for "make the schema look like the upstream".
///
/// `--dry-run` keeps the DB-free preview path: it only fetches the same
/// upstream payloads the helpers would consume and prints what would land,
/// so an operator can validate a feed before letting it touch Postgres.
async fn seed_tournament(config: &Config, competition: &str, dry_run: bool) -> anyhow::Result<()> {
    // Fail fast on unsupported competitions for the write path. `--dry-run`
    // is allowed for any competition since it only inspects upstream data.
    if !dry_run && competition != V1_TOURNAMENT_EXTERNAL_ID {
        return Err(anyhow!(
            "v1 only supports the WC competition for full seeding; got `{competition}` \
             (use --dry-run to preview without writes)"
        ));
    }

    let client = build_client(config)?;

    if dry_run {
        return run_dry_run(&client, competition).await;
    }

    let pool = init_pool(config.database_url.expose())
        .await
        .context("connecting to Postgres")?;
    let tournament_repo = PgTournamentRepository::new(pool.clone());
    let group_repo = PgTournamentGroupRepository::new(pool.clone());
    let phase_repo = PgKnockoutPhaseRepository::new(pool.clone());
    let team_repo = PgTeamRepository::new(pool.clone());

    let tournament = ensure_tournament(&tournament_repo).await?;
    let structure =
        ensure_tournament_structure(&client, &group_repo, &phase_repo, &tournament).await?;
    info!(
        groups = structure.groups.len(),
        knockouts = structure.knockouts.len(),
        "tournament structure seeded"
    );

    let teams = ensure_teams_and_assignments_seeded(
        &client,
        &team_repo,
        &group_repo,
        &tournament,
        &structure,
    )
    .await?;

    println!(
        "Seed complete: {} teams in catalog, {} groups, {} knockout phases.",
        teams.len(),
        structure.groups.len(),
        structure.knockouts.len(),
    );
    Ok(())
}

/// Fetch the same upstream payloads the seed would consume and print a
/// human-friendly preview, without ever opening a DB connection.
async fn run_dry_run(client: &SportsClient, competition: &str) -> anyhow::Result<()> {
    info!(competition, "fetching teams from upstream (dry run)");
    let teams_payload = client
        .get_competition_teams(competition)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("fetching teams for {competition}"))?;

    info!(competition, "fetching matches from upstream (dry run)");
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

    print_seed_summary(&teams_payload.teams, &assignments)
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
