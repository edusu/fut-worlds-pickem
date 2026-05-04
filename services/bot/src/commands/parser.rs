//! Slash-command parsing.
//!
//! Telegram delivers commands either as `/cmd` (in DMs) or `/cmd@BotName`
//! (in groups, especially when more than one bot is present). The parser
//! normalizes both forms into a single `Command` variant and rejects
//! commands addressed to another bot.

/// All slash commands the bot exposes. Order mirrors the canonical list in
/// `CLAUDE.md`. Adding a variant forces the dispatcher's `match` to be
/// updated (exhaustiveness check).
// Variants are referenced from `dispatch`, which itself is `#[allow(dead_code)]`
// until the update loop wires it up in feature #1; the lint will fire again
// the moment a real caller appears.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Start,
    NewPickem,
    Join,
    Play,
    Ranking,
    Help,
}

impl Command {
    /// Parse the leading slash command from a message body.
    ///
    /// Accepts:
    /// - `/cmd` — bare form, used in DMs and unambiguous groups.
    /// - `/cmd@BotName` — group form when multiple bots are present;
    ///   only matches when `BotName` equals `bot_username` (case-insensitive).
    /// - Trailing arguments after a whitespace are ignored — the parser
    ///   only inspects the command token itself.
    ///
    /// Returns `None` for non-commands, unknown commands, and commands
    /// addressed to a different bot.
    #[allow(dead_code)] // called from `dispatch` (also stubbed) and from tests
    pub fn parse(text: &str, bot_username: &str) -> Option<Self> {
        // Must start with a slash and have at least one character after it.
        let rest = text.strip_prefix('/')?;
        // The command token ends at the first whitespace; everything after
        // is treated as arguments and is irrelevant to the routing decision.
        let token = rest.split_whitespace().next()?;

        // Strip the optional `@BotName` suffix. If the suffix names another
        // bot, the message is not for us — bail out.
        let name = match token.split_once('@') {
            Some((cmd, suffix)) => {
                if suffix.eq_ignore_ascii_case(bot_username) {
                    cmd
                } else {
                    return None;
                }
            }
            None => token,
        };

        match name {
            "start" => Some(Self::Start),
            "new_pickem" => Some(Self::NewPickem),
            "join" => Some(Self::Join),
            "play" => Some(Self::Play),
            "ranking" => Some(Self::Ranking),
            "help" => Some(Self::Help),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: &str = "MyPickemBot";

    #[test]
    fn parses_bare_commands() {
        assert_eq!(Command::parse("/start", BOT), Some(Command::Start));
        assert_eq!(Command::parse("/new_pickem", BOT), Some(Command::NewPickem));
        assert_eq!(Command::parse("/join", BOT), Some(Command::Join));
        assert_eq!(Command::parse("/play", BOT), Some(Command::Play));
        assert_eq!(Command::parse("/ranking", BOT), Some(Command::Ranking));
        assert_eq!(Command::parse("/help", BOT), Some(Command::Help));
    }

    #[test]
    fn parses_addressed_commands() {
        assert_eq!(
            Command::parse("/new_pickem@MyPickemBot", BOT),
            Some(Command::NewPickem)
        );
        // Telegram normalizes mentions case-insensitively.
        assert_eq!(
            Command::parse("/join@mypickembot", BOT),
            Some(Command::Join)
        );
    }

    #[test]
    fn rejects_other_bots() {
        assert_eq!(Command::parse("/start@OtherBot", BOT), None);
    }

    #[test]
    fn ignores_trailing_arguments() {
        assert_eq!(
            Command::parse("/new_pickem some args here", BOT),
            Some(Command::NewPickem)
        );
        assert_eq!(
            Command::parse("/help@MyPickemBot please", BOT),
            Some(Command::Help)
        );
    }

    #[test]
    fn rejects_non_commands() {
        assert_eq!(Command::parse("hello", BOT), None);
        assert_eq!(Command::parse("", BOT), None);
        assert_eq!(Command::parse("/", BOT), None);
        assert_eq!(Command::parse("/unknown", BOT), None);
    }
}
