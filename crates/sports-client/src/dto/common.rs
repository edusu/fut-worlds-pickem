//! DTO fragments shared across more than one upstream endpoint.

use serde::{Deserialize, Serialize};

/// Team reference embedded inside a match or a standings row. Lighter than
/// the standalone `TeamDto` (no `area`, no `coach`) because the matches and
/// standings endpoints do not ship those.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRefDto {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub tla: Option<String>,
    pub crest: Option<String>,
}
