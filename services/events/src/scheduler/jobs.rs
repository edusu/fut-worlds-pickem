//! Cron job registrations.
//!
//! Each job here is a closure that the `tokio-cron-scheduler` runs on its
//! schedule. Keep job bodies small — call out into other modules for the real
//! work so jobs stay testable.

// TODO: register jobs:
//   - every minute: scan rounds with deadline_at within the next 60 minutes
//     and not yet flagged "approaching"; emit RoundDeadlineApproaching.
//   - every minute: scan rounds with deadline_at <= now and state='open',
//     flip to 'closed' and emit RoundClosed.
