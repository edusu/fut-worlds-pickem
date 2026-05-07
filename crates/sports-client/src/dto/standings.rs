//! DTOs for `/v4/competitions/{code}/standings`.

use serde::{Deserialize, Serialize};

use crate::dto::common::TeamRefDto;

/// Top-level shape of `/v4/competitions/{code}/standings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompetitionStandings {
    pub standings: Vec<StandingDto>,
}

/// One standings table. For the World Cup group stage there is one entry per
/// group (`group = Some("GROUP_A")`); for league competitions `group` is
/// `None` and `type` discriminates `TOTAL` / `HOME` / `AWAY`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingDto {
    pub stage: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub group: Option<String>,
    pub table: Vec<StandingsRowDto>,
}

/// Single row inside a standings table. We only consume `team` (to learn
/// which teams belong to the group); the rest is kept around for future
/// dashboards / debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingsRowDto {
    pub position: Option<i32>,
    pub team: TeamRefDto,
    pub played_games: Option<i32>,
    pub won: Option<i32>,
    pub draw: Option<i32>,
    pub lost: Option<i32>,
    pub points: Option<i32>,
    pub goals_for: Option<i32>,
    pub goals_against: Option<i32>,
    pub goal_difference: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snippet of `/v4/competitions/{code}/standings` with a single group
    /// table — the shape we expect to receive once the World Cup starts.
    /// Confirms `stage` / `type` / `group` / `table` all line up with our DTOs.
    #[test]
    fn parses_competition_standings_group_stage() {
        let json = r#"
        {
          "standings": [
            {
              "stage": "GROUP_STAGE",
              "type": "TOTAL",
              "group": "GROUP_A",
              "table": [
                {
                  "position": 1,
                  "team": { "id": 1843, "name": "Mexico", "tla": "MEX" },
                  "playedGames": 3,
                  "won": 2,
                  "draw": 1,
                  "lost": 0,
                  "points": 7,
                  "goalsFor": 5,
                  "goalsAgainst": 2,
                  "goalDifference": 3
                }
              ]
            }
          ]
        }
        "#;
        let parsed: CompetitionStandings = serde_json::from_str(json).expect("standings payload");
        assert_eq!(parsed.standings.len(), 1);
        let s = &parsed.standings[0];
        assert_eq!(s.stage.as_deref(), Some("GROUP_STAGE"));
        assert_eq!(s.type_.as_deref(), Some("TOTAL"));
        assert_eq!(s.group.as_deref(), Some("GROUP_A"));
        assert_eq!(s.table.len(), 1);
        let row = &s.table[0];
        assert_eq!(row.position, Some(1));
        assert_eq!(row.team.tla.as_deref(), Some("MEX"));
        assert_eq!(row.points, Some(7));
        assert_eq!(row.goal_difference, Some(3));
    }
}
