//! Chat automod: spots spammers in the messages the chat stream delivers and says who gets a timeout.
//! The owner, moderators, members and verified channels are never touched, and neither are commands.
//! YouTube's own filters (YouTube Studio > Settings > Community: block links, hold potentially inappropriate
//! messages) run first and cost no quota; this catches what they let through, within the daily chat budget.
use crate::chat::StreamMessage;
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

pub const TIMEOUT_SECONDS: i64 = 300;
const MEMORY_MS: i64 = 120_000;
// The same message again and again from one viewer.
const REPEAT_WINDOW_MS: i64 = 120_000;
const REPEATS: usize = 3;
const SHORT_REPEATS: usize = 5;
const SHORT_TEXT: usize = 10;
// Many messages in a row from one viewer.
const FLOOD_WINDOW_MS: i64 = 30_000;
const FLOOD_MESSAGES: usize = 8;
// One long message pasted by several viewers: a bot raid. Short ones ("lets go", "W") are hype, not spam.
const RAID_WINDOW_MS: i64 = 60_000;
const RAID_AUTHORS: usize = 3;
const RAID_TEXT: usize = 25;

static SPAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\bsub\s?(4|for)\s?sub\b|\bcheck (out )?my (channel|stream|profile|videos?)\b|\b(visit|watch) my (channel|stream)\b|\bsubscribe to (me|my)\b|\bfree (robux|v-?bucks|nitro|skins?)\b|\b(t|wa)\.me/|\btelegram\s*[:@]|\bwhats\s?app\s*[:+]?\s*\+?\d|\bonlyfans\b|\bairdrop\b|\bdouble your (btc|bitcoin|crypto|money)\b|\b(earn|make) \$?\d+[k]? (a|per) (day|week)\b",
    )
    .unwrap()
});

#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub message_id: String,
    pub author_id: String,
    pub author: String,
    pub reason: &'static str,
    /// A scam or self-promotion message is also deleted, not just its author timed out.
    pub delete: bool,
}

struct Seen {
    at: i64,
    author_id: String,
    text: String,
}

#[derive(Default)]
pub struct Automod {
    recent: VecDeque<Seen>,
    timed_out: HashMap<String, i64>,
}

// Lower case, letters and digits only, so "GTA 6 !!!" and "gta6" count as the same message.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl Automod {
    pub fn check(&mut self, message: &StreamMessage, at: i64) -> Option<Action> {
        self.recent.retain(|seen| at - seen.at < MEMORY_MS);
        self.timed_out
            .retain(|_, until| *until > at - TIMEOUT_SECONDS * 1000);
        let text = message.text.trim();
        if message.kind != 1
            || message.trusted
            || message.author_id.is_empty()
            || text.starts_with('!')
            || self.timed_out.contains_key(&message.author_id)
        {
            return None;
        }
        let normalized = normalize(text);
        self.recent.push_back(Seen {
            at,
            author_id: message.author_id.clone(),
            text: normalized.clone(),
        });
        let mine = || {
            self.recent
                .iter()
                .filter(|seen| seen.author_id == message.author_id)
        };
        let (reason, delete) = if SPAM_RE.is_match(text) {
            ("scam or self-promotion", true)
        } else if !normalized.is_empty()
            && mine()
                .filter(|seen| seen.text == normalized && at - seen.at < REPEAT_WINDOW_MS)
                .count()
                >= if normalized.chars().count() < SHORT_TEXT {
                    SHORT_REPEATS
                } else {
                    REPEATS
                }
        {
            ("the same message again and again", false)
        } else if mine().filter(|seen| at - seen.at < FLOOD_WINDOW_MS).count() >= FLOOD_MESSAGES {
            ("too many messages in a row", false)
        } else if normalized.chars().count() >= RAID_TEXT && {
            let mut authors: Vec<&str> = self
                .recent
                .iter()
                .filter(|seen| seen.text == normalized && at - seen.at < RAID_WINDOW_MS)
                .map(|seen| seen.author_id.as_str())
                .collect();
            authors.sort_unstable();
            authors.dedup();
            authors.len() >= RAID_AUTHORS
        } {
            ("a message pasted by several accounts", false)
        } else {
            return None;
        };
        self.timed_out.insert(message.author_id.clone(), at);
        Some(Action {
            message_id: message.id.clone(),
            author_id: message.author_id.clone(),
            author: message.author.clone(),
            reason,
            delete,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(author: &str, text: &str) -> StreamMessage {
        StreamMessage {
            id: format!("{author}-{text}"),
            kind: 1,
            text: text.into(),
            published_at: String::new(),
            author_id: author.into(),
            author: author.into(),
            trusted: false,
        }
    }

    #[test]
    fn scams_and_self_promotion_are_timed_out_and_deleted() {
        let mut automod = Automod::default();
        for text in [
            "sub4sub anyone?",
            "Check out my channel for GTA 6 leaks",
            "FREE ROBUX at robux-gift",
            "join t.me/gta6free",
            "whatsapp +33 6 12 34",
        ] {
            let action = automod.check(&said(text, text), 0).expect(text);
            assert!(action.delete, "{text}");
        }
        assert!(
            automod
                .check(&said("fan", "my channel is about GTA too"), 0)
                .is_none()
        );
    }

    #[test]
    fn the_owner_moderators_members_and_commands_are_left_alone() {
        let mut automod = Automod::default();
        let mut moderator = said("mod", "check out my channel");
        moderator.trusted = true;
        assert!(automod.check(&moderator, 0).is_none());
        for at in 0..10 {
            assert!(automod.check(&said("viewer", "!gta"), at * 1000).is_none());
        }
    }

    #[test]
    fn repeats_and_floods_are_timed_out_once() {
        let mut automod = Automod::default();
        let text = "when is gta 6 coming to pc";
        assert!(automod.check(&said("a", text), 0).is_none());
        assert!(automod.check(&said("a", text), 10_000).is_none());
        let action = automod.check(&said("a", text), 20_000).unwrap();
        assert_eq!(action.reason, "the same message again and again");
        assert!(!action.delete);
        assert!(
            automod.check(&said("a", text), 21_000).is_none(),
            "already timed out"
        );
        // Short hype is allowed a few more times.
        for at in 0..4 {
            assert!(automod.check(&said("b", "W"), at * 1000).is_none());
        }
        assert!(automod.check(&said("b", "W"), 5_000).is_some());
        for at in 0..7 {
            assert!(
                automod
                    .check(&said("c", &format!("message {at}")), at * 1000)
                    .is_none()
            );
        }
        assert_eq!(
            automod
                .check(&said("c", "message 7"), 7_000)
                .unwrap()
                .reason,
            "too many messages in a row"
        );
    }

    #[test]
    fn a_long_message_pasted_by_several_accounts_is_a_raid_but_hype_is_not() {
        let mut automod = Automod::default();
        let raid = "GTA 6 PC download link in my bio, works 100%";
        assert!(automod.check(&said("x", raid), 0).is_none());
        assert!(automod.check(&said("y", raid), 1_000).is_none());
        assert_eq!(
            automod.check(&said("z", raid), 2_000).unwrap().reason,
            "a message pasted by several accounts"
        );
        for (at, author) in ["d", "e", "f", "g", "h"].into_iter().enumerate() {
            assert!(
                automod
                    .check(&said(author, "LETS GOOO"), at as i64 * 1000)
                    .is_none()
            );
        }
    }
}
