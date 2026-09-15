//! Bot Chat identity policy (entity layer, PLAN §4 T16a): one bot = one
//! Hermes profile = ONE canonical forever-chat identified by
//! `(profile, session titled exactly "Bot Chat")`, resolved by
//! list-before-create — the Desktop Bot Mode invariant, no second identity.
//!
//! Pure: no I/O, no clock, no framework — the adapters (`rpc::api`) and the
//! use case (`core`) translate the wire around it. `tests/layering_guard.rs`
//! keeps it that way.

/// The exact canonical title (case-sensitive, Desktop Bot Mode invariant).
pub const BOT_CHAT_TITLE: &str = "Bot Chat";

/// Outcome of the list-before-create resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotChatResolution {
    /// A session titled exactly "Bot Chat" exists: resume THAT id — never
    /// create a second canonical chat. Recency must not win.
    Resume { stored_id: String },
    /// No canonical chat for this profile: create it.
    Create,
}

/// Resolve the canonical Bot Chat from a `session.list` result.
///
/// `rows` are `(title, durable_id)` pairs in server order. The FIRST exact
/// (case-sensitive) title match wins — the gateway's UNIQUE(title) is
/// supposed to keep one row, and if several survive, resuming the first is
/// the pinned rule. A row without a durable id could never be resumed, so it
/// is skipped. An empty list (or no match) means create.
pub fn resolve_bot_chat<'a, I>(rows: I) -> BotChatResolution
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    for (title, id) in rows {
        if title == BOT_CHAT_TITLE && !id.is_empty() {
            return BotChatResolution::Resume { stored_id: id.to_string() };
        }
    }
    BotChatResolution::Create
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_canonical_chat_resumes_not_creates() {
        let rows = [("Daily work", "s-1"), (BOT_CHAT_TITLE, "s-bot"), ("Misc", "s-3")];
        assert_eq!(
            resolve_bot_chat(rows),
            BotChatResolution::Resume { stored_id: "s-bot".into() },
            "the canonical chat is resumed, never re-created"
        );
    }

    #[test]
    fn first_exact_match_wins_when_several_survive() {
        let rows = [(BOT_CHAT_TITLE, "s-first"), (BOT_CHAT_TITLE, "s-second")];
        assert_eq!(
            resolve_bot_chat(rows),
            BotChatResolution::Resume { stored_id: "s-first".into() },
            "server order decides; the gateway UNIQUE(title) keeps one"
        );
    }

    #[test]
    fn near_matches_do_not_win() {
        // Recency (a similarly-titled chat in first position) must not win:
        // only the EXACT case-sensitive title identifies the canonical chat.
        let rows = [("bot chat", "s-lower"), ("Bot Chat ", "s-trailing"), ("BotChat", "s-glued")];
        assert_eq!(resolve_bot_chat(rows), BotChatResolution::Create);
    }

    #[test]
    fn id_less_row_is_skipped() {
        // A title match without a durable id can never be resumed: it must
        // not short-circuit the scan (a later valid row still wins).
        let rows = [(BOT_CHAT_TITLE, ""), ("Other", "s-x"), (BOT_CHAT_TITLE, "s-real")];
        assert_eq!(
            resolve_bot_chat(rows),
            BotChatResolution::Resume { stored_id: "s-real".into() }
        );
        // Only an id-less match: create.
        assert_eq!(resolve_bot_chat([(BOT_CHAT_TITLE, "")]), BotChatResolution::Create);
    }

    #[test]
    fn empty_list_creates() {
        assert_eq!(resolve_bot_chat([]), BotChatResolution::Create);
        assert_eq!(
            resolve_bot_chat([("Daily work", "s-1")]),
            BotChatResolution::Create,
            "a plain session with another title is not a Bot Chat"
        );
    }
}
