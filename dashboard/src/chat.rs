use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use prost::Message;
use serde_json::{Value, json};
use std::sync::LazyLock;
use tonic::{
    Request,
    transport::{Channel, ClientTlsConfig},
};

#[derive(Clone, PartialEq, Message)]
pub struct ListRequest {
    #[prost(string, optional, tag = "1")]
    pub live_chat_id: Option<String>,
    #[prost(string, optional, tag = "99")]
    pub page_token: Option<String>,
    #[prost(string, repeated, tag = "100")]
    pub part: Vec<String>,
}
#[derive(Clone, PartialEq, Message)]
pub struct ListResponse {
    #[prost(string, optional, tag = "2")]
    pub offline_at: Option<String>,
    #[prost(string, optional, tag = "100602")]
    pub next_page_token: Option<String>,
    #[prost(message, repeated, tag = "1007")]
    pub items: Vec<LiveChatMessage>,
}
#[derive(Clone, PartialEq, Message)]
pub struct LiveChatMessage {
    #[prost(string, optional, tag = "101")]
    pub id: Option<String>,
    #[prost(message, optional, tag = "2")]
    pub snippet: Option<Snippet>,
    #[prost(message, optional, tag = "3")]
    pub author_details: Option<AuthorDetails>,
}
#[derive(Clone, PartialEq, Message)]
pub struct Snippet {
    #[prost(int32, optional, tag = "1")]
    pub r#type: Option<i32>,
    #[prost(string, optional, tag = "4")]
    pub published_at: Option<String>,
    #[prost(string, optional, tag = "16")]
    pub display_message: Option<String>,
}
#[derive(Clone, PartialEq, Message)]
pub struct AuthorDetails {
    #[prost(string, optional, tag = "10101")]
    pub channel_id: Option<String>,
    #[prost(string, optional, tag = "103")]
    pub display_name: Option<String>,
}

pub struct StreamMessage {
    pub id: String,
    pub kind: i32,
    pub text: String,
    pub published_at: String,
    pub author_id: String,
    pub author: String,
}

pub async fn open_stream(
    key: &str,
    chat_id: &str,
    token: Option<&str>,
) -> Result<(tonic::Streaming<ListResponse>, Channel)> {
    let channel = tonic::transport::Endpoint::from_static("https://youtube.googleapis.com")
        .tls_config(ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;
    let mut grpc = tonic::client::Grpc::new(channel.clone());
    grpc.ready()
        .await
        .context("YouTube chat gRPC channel not ready")?;
    let mut request = Request::new(ListRequest {
        live_chat_id: Some(chat_id.to_owned()),
        page_token: token.map(str::to_owned),
        part: vec!["id".into(), "snippet".into(), "authorDetails".into()],
    });
    request
        .metadata_mut()
        .insert("x-goog-api-key", key.parse()?);
    let response = grpc
        .server_streaming(
            request,
            http::uri::PathAndQuery::from_static(
                "/youtube.api.v3.V3DataLiveChatMessageService/StreamList",
            ),
            tonic_prost::ProstCodec::default(),
        )
        .await?;
    Ok((response.into_inner(), channel))
}

pub fn messages(response: ListResponse) -> Vec<StreamMessage> {
    response
        .items
        .into_iter()
        .map(|item| {
            let snippet = item.snippet.unwrap_or_default();
            let author = item.author_details.unwrap_or_default();
            StreamMessage {
                id: item.id.unwrap_or_default(),
                kind: snippet.r#type.unwrap_or_default(),
                text: snippet.display_message.unwrap_or_default(),
                published_at: snippet.published_at.unwrap_or_default(),
                author_id: author.channel_id.unwrap_or_default(),
                author: author.display_name.unwrap_or_default(),
            }
        })
        .collect()
}

const STAGES: [&str; 5] = [
    "Does not convert",
    "Stops before first frame",
    "Logos or loading",
    "Menu",
    "In game",
];
const DISCORD: &str = "discord.gg/BHFztBPUe";
pub const CHAT_REMINDER: &str = "💬 Ask the stream: !game <name>, !lib <name>, !next, !top, !random. The answer shows up on screen. Discord: https://discord.gg/BHFztBPUe";
static COMMAND_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)^!(game|lib|help|discord|next|top|random)\b\s*(.*)$").unwrap()
});

fn roman_digit(word: &str) -> &str {
    match word {
        "xvi" => "16",
        "xv" => "15",
        "xiv" => "14",
        "xiii" => "13",
        "xii" => "12",
        "xi" => "11",
        "x" => "10",
        "ix" => "9",
        "viii" => "8",
        "vii" => "7",
        "vi" => "6",
        "v" => "5",
        "iv" => "4",
        "iii" => "3",
        "ii" => "2",
        _ => word,
    }
}

fn normalize(text: &str) -> String {
    let lower = text.to_lowercase().replace(['\'', '’'], "");
    let mut out = String::new();
    let mut space = true;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    let words = out.trim();
    let mut normalized = String::with_capacity(words.len());
    for word in words.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(roman_digit(word));
    }
    normalized
}
fn words(text: &str) -> Vec<String> {
    normalize(text)
        .split_whitespace()
        .filter(|s| s.len() > 2)
        .map(str::to_owned)
        .collect()
}
fn expand(mut query: String) -> String {
    for (prefix, full) in [
        ("gta", "grand theft auto"),
        ("rdr", "red dead redemption"),
        ("mgs", "metal gear solid"),
        ("gow", "god of war"),
        ("ff", "final fantasy"),
        ("re", "resident evil"),
        ("cod", "call of duty"),
        ("tlou", "the last of us"),
    ] {
        if query == prefix
            || query.starts_with(&format!("{prefix} "))
            || query
                .strip_prefix(prefix)
                .is_some_and(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()))
        {
            let suffix = query.strip_prefix(prefix).unwrap_or_default();
            query = if suffix.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                format!("{full} {suffix}")
            } else {
                format!("{full}{suffix}")
            };
            break;
        }
    }
    query.trim().to_owned()
}
fn score(query: &str, name: &str) -> u8 {
    let name = normalize(name);
    if name == query {
        4
    } else if name.starts_with(query) {
        3
    } else if name.contains(query) {
        2
    } else if query
        .split_whitespace()
        .all(|word| name.split_whitespace().any(|part| part == word))
    {
        1
    } else {
        0
    }
}
fn n(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let grouped = digits
        .chars()
        .rev()
        .enumerate()
        .fold(String::new(), |mut out, (i, ch)| {
            if i > 0 && i % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
            out
        })
        .chars()
        .rev()
        .collect::<String>();
    if value < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}
fn miss(
    id: &str,
    kicker: &str,
    what: &str,
    query: &str,
    names: impl Iterator<Item = String>,
) -> Value {
    let wanted = words(query);
    let closest = names
        .filter_map(|name| {
            let shared = words(&name).iter().filter(|w| wanted.contains(w)).count();
            (shared > 0).then_some((shared, name))
        })
        .max_by_key(|x| x.0)
        .map(|x| x.1);
    json!({"id":id,"kind":"answer","kicker":kicker,"title":format!("No {what} found with that name"),"meta":closest.map(|name|format!("Closest: {name}")).unwrap_or_else(||format!("Try part of the {what}'s name")),"icon":"🔍"})
}
fn game_card(id: &str, kicker: &str, thread: &Value) -> Value {
    let stage = thread["stage"].as_str().unwrap_or("");
    let progress = STAGES
        .iter()
        .position(|s| *s == stage)
        .map(|x| ((x + 1) * 100 / STAGES.len()) as f64)
        .unwrap_or(0.0);
    let title_id = thread["titleId"].as_str().unwrap_or("");
    let replies = thread["replies"].as_i64().unwrap_or(0);
    let meta = [
        (!title_id.is_empty()).then_some(title_id.to_owned()),
        Some(format!(
            "{} {}",
            n(replies),
            if replies == 1 { "reply" } else { "replies" }
        )),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    json!({"id":id,"kind":"answer game","kicker":kicker,"title":thread["name"],"meta":meta,"image":thread.get("cover").filter(|v|!v.is_null()),"icon":"🎮","pill":if stage.is_empty(){"Being tested"}else{stage},"progress":progress})
}
fn game_answer(id: &str, kicker: &str, query: &str, threads: &[Value]) -> Value {
    let wanted = expand(normalize(query));
    let found = threads
        .iter()
        .filter_map(|thread| {
            let value = score(&wanted, thread["name"].as_str().unwrap_or(""))
                .max(score(&wanted, thread["titleId"].as_str().unwrap_or("")));
            (value > 0).then_some((thread, value))
        })
        .max_by_key(|(thread, s)| {
            (
                *s,
                STAGES
                    .iter()
                    .position(|x| Some(*x) == thread["stage"].as_str())
                    .unwrap_or(0),
                thread["replies"].as_i64().unwrap_or(0),
            )
        })
        .map(|x| x.0);
    if (wanted == "grand theft auto 6" || wanted.starts_with("grand theft auto 6 "))
        && found.is_none()
    {
        let days = (DateTime::parse_from_rfc3339("2026-11-19T00:00:00Z")
            .unwrap()
            .timestamp_millis()
            - Utc::now().timestamp_millis())
        .div_euclid(86_400_000);
        let meta = if days > 0 {
            format!(
                "Out in {days} day{} on consoles, so there's nothing to test yet",
                if days == 1 { "" } else { "s" }
            )
        } else {
            "Out on consoles, no AnyPS5 test report yet".into()
        };
        return json!({"id":id,"kind":"answer game","kicker":kicker,"title":"GTA VI","meta":meta,"icon":"🌴","pill":"0%","progress":0});
    }
    found
        .map(|thread| game_card(id, kicker, thread))
        .unwrap_or_else(|| {
            miss(
                id,
                kicker,
                "game",
                query,
                threads
                    .iter()
                    .map(|x| x["name"].as_str().unwrap_or("").to_owned()),
            )
        })
}
fn answer(
    id: &str,
    text: &str,
    author: &str,
    threads: &[Value],
    libraries: &[Value],
    progress: &Value,
    top: &[Value],
) -> Option<Value> {
    let caps = COMMAND_RE.captures(text.trim())?;
    let command = caps[1].to_lowercase();
    let query = caps
        .get(2)
        .map(|x| x.as_str().trim().chars().take(40).collect::<String>())
        .unwrap_or_default();
    let kicker = format!(
        "!{command} · asked by {}",
        author.chars().take(30).collect::<String>()
    );
    let simple = |kind: &str, title: String, meta: String, icon: &str| json!({"id":id,"kind":kind,"kicker":kicker,"title":title,"meta":meta,"icon":icon});
    Some(match command.as_str() {
        "discord" => {
            json!({"id":id,"kind":"discord","kicker":kicker,"title":"Join the AnyPS5 Discord","meta":format!("{DISCORD} · scan the code"),"image":"discord-qr.svg"})
        }
        "help" => simple(
            "cta",
            "!game  !lib  !next  !top  !random  !discord".into(),
            "Try !game elden ring: the answer shows up here".into(),
            "💬",
        ),
        "next" => {
            if progress.is_null() {
                simple(
                    "answer",
                    "Progress is still loading".into(),
                    "Try again in a minute".into(),
                    "⏳",
                )
            } else {
                let done = progress["done"].as_i64().unwrap_or(0);
                let target = (done / 1000 + 1) * 1000;
                let left = target - done;
                let days = progress["week"]["done"]
                    .as_i64()
                    .filter(|x| *x > 0)
                    .map(|w| ((left as f64 / (w as f64 / 7.0)).round() as i64).max(1));
                json!({"id":id,"kind":"answer","kicker":kicker,"title":format!("Next goal: {} functions",n(target)),"meta":format!("{} to go{}",n(left),days.map(|d|format!(" · about {d} day{}",if d==1{""}else{"s"})).unwrap_or_default()),"icon":"🎯","progress":(done%1000) as f64/10.0})
            }
        }
        "top" => {
            if top.is_empty() {
                simple(
                    "answer",
                    "No merged PRs counted yet".into(),
                    "Try again in a few minutes".into(),
                    "⏳",
                )
            } else {
                let first = &top[0];
                let rest = top
                    .iter()
                    .skip(1)
                    .take(2)
                    .map(|x| {
                        format!(
                            "{} ({})",
                            x["author"].as_str().unwrap_or(""),
                            n(x["merged"].as_i64().unwrap_or(0))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let count = first["merged"].as_i64().unwrap_or(0);
                json!({"id":id,"kind":"answer","kicker":kicker,"title":format!("{} leads today",first["author"].as_str().unwrap_or("")),"meta":if rest.is_empty(){"Most merged PRs in 24 hours".into()}else{format!("Then {rest}")},"icon":"🏆","pill":format!("{} PR{}",n(count),if count==1{""}else{"s"})})
            }
        }
        "random" => {
            let pool: Vec<_> = threads.iter().filter(|x| x["stage"] == "In game").collect();
            if pool.is_empty() {
                simple(
                    "cta",
                    "!game  !lib  !next  !top  !random  !discord".into(),
                    "Try !game elden ring: the answer shows up here".into(),
                    "💬",
                )
            } else {
                let index = (Utc::now().timestamp_nanos_opt().unwrap_or(0).unsigned_abs() as usize)
                    % pool.len();
                game_card(id, &kicker, pool[index])
            }
        }
        "game" if normalize(&query).is_empty() => simple(
            "cta",
            "!game  !lib  !next  !top  !random  !discord".into(),
            "Try !game elden ring: the answer shows up here".into(),
            "💬",
        ),
        "lib" if normalize(&query).is_empty() => simple(
            "cta",
            "!game  !lib  !next  !top  !random  !discord".into(),
            "Try !game elden ring: the answer shows up here".into(),
            "💬",
        ),
        "game" => game_answer(id, &kicker, &query, threads),
        "lib" => {
            let wanted = normalize(&query);
            let found = libraries
                .iter()
                .filter_map(|lib| {
                    let s = score(&wanted, lib["name"].as_str().unwrap_or(""))
                        .max(score(&wanted, lib["label"].as_str().unwrap_or("")));
                    (s > 0).then_some((lib, s))
                })
                .max_by_key(|(lib, s)| (*s, lib["total"].as_i64().unwrap_or(0)))
                .map(|x| x.0);
            if let Some(lib) = found {
                let done = lib["done"].as_i64().unwrap_or(0);
                let total = lib["total"].as_i64().unwrap_or(0);
                let pct = if total > 0 { done * 100 / total } else { 0 };
                json!({"id":id,"kind":"answer","kicker":kicker,"title":lib["label"],"meta":format!("{} / {} functions implemented",n(done),n(total)),"icon":"📚","pill":format!("{pct}%"),"progress":pct})
            } else {
                miss(
                    id,
                    &kicker,
                    "library",
                    &query,
                    libraries
                        .iter()
                        .map(|x| x["label"].as_str().unwrap_or("").to_owned()),
                )
            }
        }
        _ => return None,
    })
}

pub fn command_answer(
    message: &StreamMessage,
    threads: &[Value],
    libraries: &[Value],
    progress: &Value,
    top: &[Value],
) -> Option<Value> {
    if message.kind != 1 {
        return None;
    }
    answer(
        &message.id,
        &message.text,
        &message.author,
        threads,
        libraries,
        progress,
        top,
    )
}

pub fn next_chat_delay(
    now_ms: i64,
    burst_until_ms: i64,
    remaining_units: i64,
    idle_seconds: u64,
) -> u64 {
    if remaining_units <= 0 {
        300_000
    } else if now_ms < burst_until_ms {
        250
    } else {
        idle_seconds * 1000
    }
}

pub fn quota_day(now_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(now_ms)
        .unwrap_or_else(Utc::now)
        .with_timezone(&chrono_tz::America::Los_Angeles)
        .date_naive()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> StreamMessage {
        StreamMessage {
            id: "m1".into(),
            kind: 1,
            text: text.into(),
            published_at: String::new(),
            author_id: "viewer-id".into(),
            author: "Viewer".into(),
        }
    }
    fn thread(name: &str, stage: &str, replies: i64, title_id: &str) -> Value {
        json!({"name":name,"stage":stage,"replies":replies,"titleId":title_id})
    }
    fn threads() -> Vec<Value> {
        vec![
            thread("Demon's Souls", "Menu", 4, "PPSA01341"),
            thread("Demon's Souls", "In game", 9, "PPSA01342"),
            thread("Astro's Playroom", "Logos or loading", 30, "PPSA01325"),
        ]
    }
    fn ask(text: &str) -> Option<Value> {
        command_answer(
            &message(text),
            &threads(),
            &[
                json!({"name":"libKernel","label":"Kernel","done":450,"total":600}),
                json!({"name":"libSceGnmDriver","label":"GPU driver","done":10,"total":400}),
            ],
            &Value::Null,
            &[],
        )
    }

    #[test]
    fn ignores_non_commands_and_near_misses() {
        for text in [
            "hello",
            "this !game is great",
            "!gamer",
            "!discordant",
            "!topic",
        ] {
            assert!(ask(text).is_none(), "{text}");
        }
        assert!(
            command_answer(
                &StreamMessage {
                    kind: 0,
                    ..message("!help")
                },
                &[],
                &[],
                &Value::Null,
                &[]
            )
            .is_none()
        );
    }
    #[test]
    fn game_matching_preserves_upstream_behavior() {
        let answer = ask("!game demons souls").unwrap();
        assert_eq!(answer["title"], "Demon's Souls");
        assert_eq!(answer["pill"], "In game");
        assert_eq!(answer["kicker"], "!game · asked by Viewer");
        assert_eq!(ask("!GAME ppsa01325").unwrap()["title"], "Astro's Playroom");
        assert_eq!(ask("!game astro").unwrap()["title"], "Astro's Playroom");
        assert_eq!(
            ask("!game playroom of astro bot").unwrap()["meta"],
            "Closest: Astro's Playroom"
        );
    }
    #[test]
    fn library_matching_and_help_are_preserved() {
        assert_eq!(ask("!lib kernel").unwrap()["pill"], "75%");
        assert_eq!(ask("!lib gnm").unwrap()["title"], "GPU driver");
        assert!(
            ask("!lib zzz").unwrap()["title"]
                .as_str()
                .unwrap()
                .starts_with("No library found")
        );
        assert!(
            ask("!help").unwrap()["title"]
                .as_str()
                .unwrap()
                .contains("!game")
        );
        assert!(
            ask("!game").unwrap()["title"]
                .as_str()
                .unwrap()
                .contains("!lib")
        );
    }
    #[test]
    fn viewer_query_never_appears_in_screen_answer() {
        let game = ask("!game buy cheap followers at spam.example")
            .unwrap()
            .to_string();
        let library = ask("!lib spam.example").unwrap().to_string();
        assert!(!game.contains("spam") && !game.contains("followers"));
        assert!(!library.contains("spam"));
    }
    #[test]
    fn aliases_word_order_and_numerals_match() {
        let games = vec![
            thread("Grand Theft Auto V", "In game", 25, ""),
            thread("Red Dead Redemption", "In game", 2, ""),
            thread("Final Fantasy VII", "In game", 1, ""),
            thread("Resident Evil 4", "Menu", 1, ""),
        ];
        let ask_games =
            |s: &str| command_answer(&message(s), &games, &[], &Value::Null, &[]).unwrap();
        for (query, title) in [
            ("!game gta v", "Grand Theft Auto V"),
            ("!game gta 5", "Grand Theft Auto V"),
            ("!game GTA5", "Grand Theft Auto V"),
            ("!game rdr", "Red Dead Redemption"),
            ("!game redemption red", "Red Dead Redemption"),
            ("!game final fantasy 7", "Final Fantasy VII"),
            ("!game ff7", "Final Fantasy VII"),
            ("!game resident evil iv", "Resident Evil 4"),
        ] {
            assert_eq!(ask_games(query)["title"], title, "{query}");
        }
    }
    #[test]
    fn roman_numerals_are_replaced_only_as_whole_words() {
        assert_eq!(normalize("Final Fantasy XVI"), "final fantasy 16");
        assert_eq!(normalize("Mix V"), "mix 5");
        assert_eq!(normalize("Resident Evil IV"), "resident evil 4");
    }
    #[test]
    fn gta_six_fallback_yields_to_a_real_thread() {
        let answer = ask("!game gta 6").unwrap();
        assert_eq!(answer["title"], "GTA VI");
        assert_eq!(answer["pill"], "0%");
        let real = command_answer(
            &message("!game gta vi"),
            &[thread("Grand Theft Auto VI", "Menu", 3, "")],
            &[],
            &Value::Null,
            &[],
        )
        .unwrap();
        assert_eq!(real["pill"], "Menu");
    }
    #[test]
    fn discord_next_top_and_random_match_contract() {
        let discord = ask("!discord").unwrap();
        assert_eq!(discord["kind"], "discord");
        assert!(
            discord["meta"]
                .as_str()
                .unwrap()
                .contains("discord.gg/BHFztBPUe")
        );
        assert_eq!(discord["image"], "discord-qr.svg");
        assert!(
            ask("!next").unwrap()["title"]
                .as_str()
                .unwrap()
                .contains("loading")
        );
        let progress = json!({"done":9134,"week":{"done":2800}});
        let top = vec![
            json!({"author":"ana","merged":18}),
            json!({"author":"bo","merged":3}),
        ];
        let next = command_answer(&message("!next"), &[], &[], &progress, &[]).unwrap();
        assert_eq!(next["title"], "Next goal: 10,000 functions");
        assert!(
            next["meta"]
                .as_str()
                .unwrap()
                .starts_with("866 to go · about 2 days")
        );
        let top_answer = command_answer(&message("!top"), &[], &[], &Value::Null, &top).unwrap();
        assert_eq!(top_answer["title"], "ana leads today");
        assert_eq!(top_answer["pill"], "18 PRs");
        for _ in 0..20 {
            assert_eq!(ask("!random").unwrap()["pill"], "In game");
        }
    }
    #[test]
    fn reminder_budget_and_california_day_contracts() {
        assert!(CHAT_REMINDER.chars().count() <= 200);
        assert!(CHAT_REMINDER.contains("https://discord.gg/BHFztBPUe"));
        assert_eq!(next_chat_delay(1000, 5000, 1, 75), 250);
        assert_eq!(next_chat_delay(6000, 5000, 1, 75), 75_000);
        assert_eq!(next_chat_delay(1000, 5000, 0, 75), 300_000);
        assert_eq!(
            quota_day(
                chrono::DateTime::parse_from_rfc3339("2026-11-02T07:00:00Z")
                    .unwrap()
                    .timestamp_millis()
            ),
            "2026-11-01"
        );
        assert_eq!(
            quota_day(
                chrono::DateTime::parse_from_rfc3339("2026-11-02T09:00:00Z")
                    .unwrap()
                    .timestamp_millis()
            ),
            "2026-11-02"
        );
    }
}
