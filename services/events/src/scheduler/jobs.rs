//! Cron job registrations.
//!
//! Each job here is a closure that the `tokio-cron-scheduler` runs on its
//! schedule. Keep job bodies small — call out into other modules for the real
//! work so jobs stay testable.

// TODO: register jobs (parent windows = tournament_groups + knockout_phases):
//   - every minute: scan parent windows with deadline_at within the next 60
//     minutes and not yet flagged "approaching"; emit
//     `SubmissionDeadlineApproaching` (carrying the parent's `ParentRef`).
//   - every minute: scan parent windows with deadline_at <= now and
//     state='open', flip to 'closed' and emit `SubmissionWindowClosed`.
