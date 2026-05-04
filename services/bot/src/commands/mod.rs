//! Slash-command handlers. One file per command keeps each handler focused
//! and easy to test in isolation. Routing of raw message text to a typed
//! variant lives in `parser`; the `dispatch` function below turns that
//! variant into the matching handler call.

pub mod help;
pub mod join;
pub mod new_pickem;
pub mod parser;
pub mod play;
pub mod ranking;
pub mod start;

pub use parser::Command;

/// Dispatch a parsed `Command` to its handler.
///
/// Today the handlers are zero-arg stubs; feature #1 will grow real
/// signatures (chat id, user id, db pool, telegram client, raw args) and
/// the dispatcher will forward those through. The exhaustive `match`
/// guarantees the compiler complains the moment a new `Command` variant
/// is added without a corresponding handler arm.
#[allow(dead_code)] // wired up by the update loop in feature #1
pub async fn dispatch(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Start => start::handle().await,
        Command::NewPickem => new_pickem::handle().await,
        Command::Join => join::handle().await,
        Command::Play => play::handle().await,
        Command::Ranking => ranking::handle().await,
        Command::Help => help::handle().await,
    }
}
