use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::user::TelegramUserId;

/// A user's prediction for a single match.
///
/// Field semantics:
/// - `reg_home` / `reg_away` are the predicted regulation 90' goals. In
///   group-stage matches this is the only score that matters; in knockout
///   matches it doubles as the "what's the score after 90'" prediction.
/// - `advancement_winner_team_id` only applies to knockout matches. For
///   non-draw regulation predictions the api may leave this `None` and let
///   the scorer derive it as the regulation winner; for draw regulation
///   predictions the user must supply it explicitly.
/// - `pens_winner_team_id` only applies when the user predicted a draw in
///   regulation AND predicts the match will go to penalties. Setting it
///   implies "yes pens"; leaving it `None` (with a draw reg prediction)
///   implies "no pens, ends in extra time".
/// - `was_changed` is set to `true` the moment any field is rewritten via
///   the api's re-pick endpoint. It does not compound on additional edits.
/// - `points_awarded` is `None` until the match is scored, allowing us to
///   tell "not yet scored" apart from "zero points".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prediction {
    pub id: Uuid,
    pub user_id: TelegramUserId,
    pub match_id: Uuid,
    pub reg_home: i32,
    pub reg_away: i32,
    pub advancement_winner_team_id: Option<Uuid>,
    pub pens_winner_team_id: Option<Uuid>,
    pub was_changed: bool,
    pub points_awarded: Option<i32>,
    pub submitted_at: DateTime<Utc>,
}

impl Prediction {
    /// Whether the regulation prediction is a draw. A draw prediction is the
    /// prerequisite for both extra-time and penalties paths.
    pub fn predicted_reg_draw(&self) -> bool {
        self.reg_home == self.reg_away
    }

    /// Predicted "match will go to extra time". Implicit from the regulation
    /// score being a draw — there is no separate user input for this.
    pub fn predicted_goes_to_et(&self) -> bool {
        self.predicted_reg_draw()
    }

    /// Predicted "match will go to penalties". Implicit from the
    /// `pens_winner_team_id` field being populated.
    pub fn predicted_goes_to_pens(&self) -> bool {
        self.pens_winner_team_id.is_some()
    }

    /// Validate the cross-field invariants enforced at the schema level.
    /// Returned as a plain `Result<(), &'static str>` so callers anywhere in
    /// the workspace can pre-check before round-tripping to Postgres.
    pub fn is_consistent(&self) -> Result<(), &'static str> {
        // Pens-winner only valid when reg prediction was a draw.
        if self.pens_winner_team_id.is_some() && !self.predicted_reg_draw() {
            return Err("pens_winner set but reg prediction is not a draw");
        }
        // Pens-winner must match advancement-winner when both set.
        if let (Some(adv), Some(pens)) = (self.advancement_winner_team_id, self.pens_winner_team_id)
        {
            if adv != pens {
                return Err("advancement_winner and pens_winner disagree");
            }
        }
        Ok(())
    }
}
