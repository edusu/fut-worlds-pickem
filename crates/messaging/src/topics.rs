//! NATS subject names. All topics are namespaced under `pickem.` so several
//! systems can share a NATS cluster without collisions.

pub const MATCH_FINISHED: &str = "pickem.match.finished";
pub const MATCH_LIVE: &str = "pickem.match.live";

pub const ROUND_DEADLINE_APPROACHING: &str = "pickem.round.deadline_approaching";
pub const ROUND_CLOSED: &str = "pickem.round.closed";
pub const ROUND_SCORED: &str = "pickem.round.scored";

pub const PREDICTIONS_SUBMITTED: &str = "pickem.predictions.submitted";

pub const NOTIFICATION_REQUESTED: &str = "pickem.notification.requested";
