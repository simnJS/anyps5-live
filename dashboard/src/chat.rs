use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use prost::Message;
use serde_json::{Value, json};
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
const ROMAN: [(&str, &str); 15] = [
    ("xvi", "16"),
    ("xv", "15"),
    ("xiv", "14"),
    ("xiii", "13"),
    ("xii", "12"),
    ("xi", "11"),
    ("x", "10"),
    ("ix", "9"),
    ("viii", "8"),
    ("vii", "7"),
    ("vi", "6"),
    ("v", "5"),
    ("iv", "4"),
    ("iii", "3"),
    ("ii", "2"),
];

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
    let mut normalized = out.trim().to_owned();
    for (roman, digit) in ROMAN {
        let re = regex::Regex::new(&format!(r"\b{roman}\b")).unwrap();
        normalized = re.replace_all(&normalized, digit).into_owned();
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
            query = format!("{full}{}", query.strip_prefix(prefix).unwrap_or_default());
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
            format!("Out in {days} days on consoles, so there's nothing to test yet")
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
    let re = regex::Regex::new(r"(?i)^!(game|lib|help|discord|next|top|random)\b\s*(.*)$").unwrap();
    let caps = re.captures(text.trim())?;
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
