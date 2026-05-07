//! DTOs for `/v4/competitions/{code}/teams`.

use serde::{Deserialize, Serialize};

/// Top-level shape of `/v4/competitions/{code}/teams`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompetitionTeams {
    pub count: Option<i32>,
    pub teams: Vec<TeamDto>,
}

/// Standalone team document. Fields beyond those used by the ingester
/// (`coach`, `squad`, `runningCompetitions`, `venue`, ...) are intentionally
/// dropped — extending the DTO is cheap when a new ingestion path needs them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamDto {
    pub id: i64,
    pub name: String,
    pub short_name: Option<String>,
    /// Three-letter FIFA-style code (e.g. "ARG", "ESP").
    pub tla: Option<String>,
    pub crest: Option<String>,
    pub area: Option<AreaDto>,
}

/// Geographical area (country, region, continent) the team belongs to.
/// For national teams this aligns 1:1 with the team itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaDto {
    pub id: Option<i64>,
    pub name: Option<String>,
    /// 3-letter FIFA / IOC-style country code. Used as `teams.country_code`.
    pub code: Option<String>,
    /// Crest URL (SVG/PNG). Not an emoji.
    pub flag: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snippet of `/v4/competitions/{code}/teams` with a national team
    /// (Argentina). Verifies `area` and its nested `code` deserialize.
    #[test]
    fn parses_competition_teams_with_area() {
        let json = r#"
        {
          "count": 1,
          "teams": [
            {
              "id": 759,
              "name": "Argentina",
              "shortName": "Argentina",
              "tla": "ARG",
              "crest": "https://crests.football-data.org/759.svg",
              "area": {
                "id": 2010,
                "name": "Argentina",
                "code": "ARG",
                "flag": "https://crests.football-data.org/759.svg"
              }
            }
          ]
        }
        "#;
        let parsed: CompetitionTeams = serde_json::from_str(json).expect("teams payload");
        assert_eq!(parsed.count, Some(1));
        assert_eq!(parsed.teams.len(), 1);
        let t = &parsed.teams[0];
        assert_eq!(t.tla.as_deref(), Some("ARG"));
        let area = t.area.as_ref().expect("area present");
        assert_eq!(area.code.as_deref(), Some("ARG"));
    }
}
