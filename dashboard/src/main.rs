use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Request, State},
    middleware::{self, Next},
    response::IntoResponse,
    routing::get,
};
use chrono::{DateTime, Utc};
mod chat;
use regex::Regex;
use reqwest::Client;
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::{
    fs,
    sync::{RwLock, Semaphore},
    task::JoinSet,
};
use tower_http::services::ServeDir;

static GITHUB_RATE_REMAINING: AtomicI64 = AtomicI64::new(-1);
static IMPLEMENTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([+−-]\s?\d[\d\s]*) implemented functions?").unwrap());
static DECLARED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([+−-]\s?\d[\d\s]*) declared functions?").unwrap());
static GAME_STAGE_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^#{2,3} Furthest point\s*\n+(.+)$").unwrap());
static GAME_STAGE_RULES: LazyLock<[(Regex, usize); 5]> = LazyLock::new(|| {
    [
        (
            Regex::new(r"\bin[- ]game\b|\bgameplay\b|\bplayable\b").unwrap(),
            4,
        ),
        (Regex::new(r"\bmenus?\b").unwrap(), 3),
        (
            Regex::new(r"\blogos?\b|loading screen|boot screens?").unwrap(),
            2,
        ),
        (
            Regex::new(r"before the first|^boots? \(?stops|^none\b").unwrap(),
            1,
        ),
        (Regex::new(r"does not convert").unwrap(), 0),
    ]
});
static GAME_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z]{4}\d{5}").unwrap());
static ROCKSTAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgta\b|red dead").unwrap());

struct CurrentState<'a> {
    data: &'a Value,
    server_time: &'a str,
}

// Add the per-request timestamp without cloning the full dashboard JSON tree.
impl Serialize for CurrentState<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let Some(data) = self.data.as_object() else {
            return Err(serde::ser::Error::custom(
                "dashboard state must be an object",
            ));
        };
        let extra = usize::from(!data.contains_key("serverTime"));
        let mut map = serializer.serialize_map(Some(data.len() + extra))?;
        for (key, value) in data {
            if key != "serverTime" {
                map.serialize_entry(key, value)?;
            }
        }
        map.serialize_entry("serverTime", self.server_time)?;
        map.end()
    }
}

fn serialize_state(
    data: &Value,
    server_time: &str,
) -> std::result::Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&CurrentState { data, server_time })
}

#[derive(Clone)]
struct AppState {
    data: Arc<RwLock<Value>>,
    persisted: Arc<RwLock<Value>>,
    data_file: PathBuf,
    client: Client,
    threads: Arc<RwLock<Arc<Vec<Value>>>>,
    covers: Arc<RwLock<HashMap<String, String>>>,
    live_chat_id: Arc<RwLock<Option<String>>>,
}

async fn state(State(app): State<AppState>) -> axum::response::Response {
    // Keep the snapshot read-only while serializing its response bytes.
    let data = app.data.read().await;
    let server_time = Utc::now().to_rfc3339();
    match serialize_state(&data, &server_time) {
        Ok(body) => {
            let mut response = axum::response::Response::new(axum::body::Body::from(body));
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json"),
            );
            response
        }
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        )
            .into_response(),
    }
}

async fn chat_state(State(app): State<AppState>) -> Json<Value> {
    Json(app.data.read().await["chat"].clone())
}

async fn short_meta(State(app): State<AppState>) -> Json<Value> {
    let data = app.data.read().await;
    let done = data["progress"]["day"]["done"].as_i64().unwrap_or(0);
    let gain = if data["progress"]["day"].is_null() {
        "progress".to_string()
    } else {
        format!("+{done} functions")
    };
    let live = env::var("YOUTUBE_VIDEO_ID")
        .ok()
        .map(|id| format!("\n\nWatch it live, 24/7: https://www.youtube.com/watch?v={id}"))
        .unwrap_or_default();
    Json(json!({
        "snippet": {
            "title": format!("PS5 games on PC with no emulator: AnyPS5 today, {gain} #Shorts"),
            "description": format!("AnyPS5 is a community project that converts PS5 games to run natively on Windows and Linux, with no emulator. Reaching gameplay does not mean a game is playable from start to finish yet. This is today's progress.{live}\n\nAn unofficial community project, not affiliated with the AnyPS5 maintainers or with Sony."),
            "tags": ["AnyPS5", "PS5", "emulation"], "categoryId": "20"
        },
        "status": {"privacyStatus": env::var("SHORT_PRIVACY").unwrap_or_else(|_| "public".into()), "selfDeclaredMadeForKids": false}
    }))
}

async fn health(State(app): State<AppState>) -> (axum::http::StatusCode, &'static str) {
    let data = app.data.read().await;
    if data["progress"].is_null() && data["github"].is_null() {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, "starting")
    } else {
        (axum::http::StatusCode::OK, "ok")
    }
}

async fn mark_source_error(data: &Arc<RwLock<Value>>, source: &str, error: &str) {
    let mut state = data.write().await;
    let at = state["sources"][source]["at"].clone();
    state["sources"][source] = json!({"ok":false,"at":at,"error":error});
    state["updatedAt"] = json!(Utc::now().to_rfc3339());
}

fn apply_cache_policy(path: &str, response: &mut axum::response::Response) {
    if matches!(path, "/api/state" | "/api/chat" | "/api/short-meta") {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
        if response.status().is_success() {
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json; charset=utf-8"),
            );
        }
    } else if path != "/healthz" && response.status().is_success() {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        );
        let content_type = if path == "/" {
            Some("text/html; charset=utf-8")
        } else if path.ends_with(".html") {
            Some("text/html; charset=utf-8")
        } else if path.ends_with(".css") {
            Some("text/css; charset=utf-8")
        } else if path.ends_with(".js") {
            Some("text/javascript; charset=utf-8")
        } else if path.ends_with(".svg") {
            Some("image/svg+xml")
        } else if path.ends_with(".png") {
            Some("image/png")
        } else if path.ends_with(".woff2") {
            Some("font/woff2")
        } else if path.ends_with(".json") {
            Some("application/json; charset=utf-8")
        } else {
            None
        };
        if let Some(content_type) = content_type {
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static(content_type),
            );
        }
    }
}

async fn cache_policy(request: Request, next: Next) -> axum::response::Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;
    apply_cache_policy(&path, &mut response);
    response
}

async fn fetch_json(client: &Client, url: &str, token: Option<&str>) -> Result<Value> {
    let mut req = client.get(url);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let response = req.send().await?;
    if response
        .headers()
        .get("x-ratelimit-resource")
        .and_then(|v| v.to_str().ok())
        != Some("search")
    {
        if let Some(remaining) = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok())
        {
            GITHUB_RATE_REMAINING.store(remaining, Ordering::Relaxed);
        }
    }
    Ok(response.error_for_status()?.json().await?)
}

fn report_counts(body: &str) -> Value {
    let read = |pattern: &Regex| {
        let value = pattern
            .captures(body)?
            .get(1)?
            .as_str()
            .replace('−', "-")
            .replace(['+', ' '], "");
        value.parse::<i64>().ok()
    };
    json!({"implemented":read(&IMPLEMENTED_RE).unwrap_or(0),"declared":read(&DECLARED_RE).unwrap_or(0)})
}

async fn fetch_report(
    client: &Client,
    repo: &str,
    number: i64,
    token: Option<&str>,
) -> Result<Option<Value>> {
    let comments = fetch_json(
        client,
        &format!("https://api.github.com/repos/{repo}/issues/{number}/comments?per_page=100"),
        token,
    )
    .await?;
    Ok(comments
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| {
            c["body"]
                .as_str()
                .unwrap_or_default()
                .contains("<!-- progress-report -->")
        })
        .map(|c| report_counts(c["body"].as_str().unwrap_or_default())))
}

async fn fetch_reports_concurrently(
    client: &Client,
    repo: &str,
    numbers: impl IntoIterator<Item = i64>,
    token: Option<&str>,
) -> HashMap<i64, Result<Option<Value>>> {
    const MAX_IN_FLIGHT: usize = 4;
    let semaphore = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let mut tasks = JoinSet::new();
    let mut results = HashMap::new();
    for number in numbers {
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(permit) => permit,
            Err(error) => {
                results.insert(number, Err(anyhow::anyhow!(error.to_string())));
                continue;
            }
        };
        let client = client.clone();
        let repo = repo.to_owned();
        let token = token.map(str::to_owned);
        tasks.spawn(async move {
            let _permit = permit;
            let report = fetch_report(&client, &repo, number, token.as_deref()).await;
            (number, report)
        });
    }
    while let Some(joined) = tasks.join_next().await {
        if let Ok((number, result)) = joined {
            results.insert(number, result);
        }
    }
    results
}

async fn youtube_stats(
    client: &Client,
    data: Arc<RwLock<Value>>,
    live_chat_id: Arc<RwLock<Option<String>>>,
) {
    let (Ok(key), Ok(video)) = (env::var("YOUTUBE_API_KEY"), env::var("YOUTUBE_VIDEO_ID")) else {
        return;
    };
    loop {
        let result = client
            .get("https://www.googleapis.com/youtube/v3/videos")
            .query(&[
                ("part", "statistics,liveStreamingDetails"),
                ("id", video.as_str()),
            ])
            .header("X-Goog-Api-Key", &key)
            .send()
            .await;
        let updated = match result {
            Ok(response) if response.status().is_success() => {
                match response.json::<Value>().await {
                    Ok(body) => {
                        if let Some(item) = body["items"].as_array().and_then(|a| a.first()) {
                            let likes = item["statistics"]["likeCount"]
                                .as_str()
                                .and_then(|v| v.parse::<u64>().ok());
                            let viewers = item["liveStreamingDetails"]["concurrentViewers"]
                                .as_str()
                                .and_then(|v| v.parse::<u64>().ok());
                            *live_chat_id.write().await =
                                item["liveStreamingDetails"]["activeLiveChatId"]
                                    .as_str()
                                    .map(str::to_owned);
                            let mut state = data.write().await;
                            state["youtube"]["likes"] = likes.map_or(Value::Null, |v| json!(v));
                            state["youtube"]["viewers"] = viewers.map_or(Value::Null, |v| json!(v));
                            state["sources"]["youtube"] =
                                json!({"ok":true,"at":Utc::now().to_rfc3339()});
                            Ok(())
                        } else {
                            Err(anyhow::anyhow!("YouTube video not found"))
                        }
                    }
                    Err(e) => Err(e.into()),
                }
            }
            Ok(response) => Err(anyhow::anyhow!("YouTube HTTP {}", response.status())),
            Err(e) => Err(e.into()),
        };
        if let Err(error) = updated {
            let mut state = data.write().await;
            let at = state["sources"]["youtube"]["at"].clone();
            state["sources"]["youtube"] = json!({"ok":false,"at":at,"error":error.to_string()});
        }
        tokio::time::sleep(Duration::from_secs(120)).await;
    }
}

const GAME_STAGES: [&str; 5] = [
    "Does not convert",
    "Stops before first frame",
    "Logos or loading",
    "Menu",
    "In game",
];
const CATEGORY_QUERY: &str = "query($owner:String!,$name:String!,$slug:String!){repository(owner:$owner,name:$name){discussionCategory(slug:$slug){id}}}";
const THREADS_QUERY: &str = "query($owner:String!,$name:String!,$category:ID!,$after:String){repository(owner:$owner,name:$name){discussions(first:100,after:$after,categoryId:$category,orderBy:{field:UPDATED_AT,direction:DESC}){pageInfo{endCursor hasNextPage}nodes{number title body createdAt updatedAt comments{totalCount}}}}}";

fn game_stage(body: &str) -> Option<usize> {
    let answer = GAME_STAGE_HEADER_RE
        .captures(body)?
        .get(1)?
        .as_str()
        .replace(['*', '`'], "")
        .trim()
        .to_lowercase();
    if answer.starts_with("- [") {
        return None;
    }
    GAME_STAGE_RULES
        .iter()
        .filter_map(|(pattern, stage)| pattern.find(&answer).map(|m| (m.start(), *stage)))
        .min_by_key(|x| x.0)
        .map(|x| x.1)
}

async fn github_graphql(
    client: &Client,
    token: &str,
    query: &str,
    variables: Value,
) -> Result<Value> {
    let response = client
        .post("https://api.github.com/graphql")
        .bearer_auth(token)
        .json(&json!({"query":query,"variables":variables}))
        .send()
        .await?
        .error_for_status()?;
    let body: Value = response.json().await?;
    if let Some(error) = body["errors"].as_array().and_then(|a| a.first()) {
        anyhow::bail!(
            "GitHub GraphQL: {}",
            error["message"].as_str().unwrap_or("no data")
        );
    }
    Ok(body["data"].clone())
}

async fn collect_games(
    client: &Client,
    data: Arc<RwLock<Value>>,
    all_threads: Arc<RwLock<Arc<Vec<Value>>>>,
    covers: Arc<RwLock<HashMap<String, String>>>,
) {
    let Some(token) = env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty()) else {
        return;
    };
    let repo = env::var("ANYPS5_REPO").unwrap_or_else(|_| "boykopovar/AnyPS5".into());
    let (owner, name) = repo.split_once('/').unwrap_or(("boykopovar", "AnyPS5"));
    let covers = covers.read().await.clone();
    let category = github_graphql(
        client,
        &token,
        CATEGORY_QUERY,
        json!({"owner":owner,"name":name,"slug":"game-bring-up"}),
    )
    .await;
    if let Err(error) = &category {
        mark_source_error(&data, "games", &error.to_string()).await;
        return;
    }
    let category_id = category.ok().and_then(|v| {
        v["repository"]["discussionCategory"]["id"]
            .as_str()
            .map(str::to_owned)
    });
    let Some(category_id) = category_id else {
        mark_source_error(
            &data,
            "games",
            "Game bring-up discussion category not found",
        )
        .await;
        return;
    };
    let mut discussions = Vec::new();
    let mut after = Value::Null;
    loop {
        let page = match github_graphql(
            client,
            &token,
            THREADS_QUERY,
            json!({"owner":owner,"name":name,"category":category_id,"after":after}),
        )
        .await
        {
            Ok(v) => v,
            Err(error) => {
                mark_source_error(&data, "games", &error.to_string()).await;
                return;
            }
        };
        let connection = &page["repository"]["discussions"];
        discussions.extend(connection["nodes"].as_array().cloned().unwrap_or_default());
        if !connection["pageInfo"]["hasNextPage"]
            .as_bool()
            .unwrap_or(false)
        {
            break;
        }
        after = connection["pageInfo"]["endCursor"].clone();
    }
    let threads: Arc<Vec<Value>> = Arc::new(discussions.iter().map(|d| {
        let title = d["title"].as_str().unwrap_or_default();
        let id = GAME_ID_RE.find(title).map(|m| m.as_str());
        let game_name = id.map(|id| title[..title.find(id).unwrap_or(0)].trim_end_matches([' ', '(', '—', '–', ':', '-']).to_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| title.to_owned());
        let stage = game_stage(d["body"].as_str().unwrap_or_default()).map(|i| GAME_STAGES[i]);
        json!({"number":d["number"],"name":game_name,"titleId":id,"stage":stage,"replies":d["comments"]["totalCount"],"createdAt":d["createdAt"],"updatedAt":d["updatedAt"],"cover":id.and_then(|id| covers.get(id))})
    }).collect());
    let count = |stage: &str| threads.iter().filter(|t| t["stage"] == stage).count();
    let now = Utc::now();
    let active = threads
        .iter()
        .filter(|t| {
            t["updatedAt"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .is_some_and(|d| now.signed_duration_since(d.with_timezone(&Utc)).num_hours() < 24)
        })
        .count();
    let mut highlights: Vec<Value> = threads
        .iter()
        .filter(|t| t["stage"] == "In game" && !t["cover"].is_null())
        .cloned()
        .collect();
    highlights.sort_by_key(|t| -t["replies"].as_i64().unwrap_or(0));
    highlights.truncate(24);
    let mut rockstar: Vec<Value> = threads
        .iter()
        .filter(|t| {
            let name = t["name"].as_str().unwrap_or("").to_lowercase();
            name.contains("grand theft auto") || ROCKSTAR_RE.is_match(&name)
        })
        .cloned()
        .collect();
    rockstar.sort_by_key(|t| -t["replies"].as_i64().unwrap_or(0));
    let games = json!({"tracked":threads.len(),"inGame":count("In game"),"menu":count("Menu"),"active24h":active,"stages":GAME_STAGES.iter().map(|stage|json!({"name":stage,"count":count(stage)})).collect::<Vec<_>>(),"recent":threads.iter().take(24).cloned().collect::<Vec<_>>(),"highlights":highlights,"rockstar":rockstar});
    *all_threads.write().await = threads;
    let mut state = data.write().await;
    state["games"] = games;
    let at = now.to_rfc3339();
    state["sources"]["games"] = json!({"ok":true,"at":at});
    state["updatedAt"] = json!(at);
}

async fn collect_covers(
    client: &Client,
    data: Arc<RwLock<Value>>,
    covers: Arc<RwLock<HashMap<String, String>>>,
) {
    let roadmap = env::var("ROADMAP_URL")
        .unwrap_or_else(|_| "https://alxcbtw.github.io/anyps5-roadmap".into());
    match fetch_json(
        client,
        &format!("{}/roadmap.json", roadmap.trim_end_matches('/')),
        None,
    )
    .await
    {
        Ok(value) => {
            let mut entries = HashMap::new();
            for game in value["games"].as_array().into_iter().flatten() {
                let Some(image) = game["image"]["wide"]
                    .as_str()
                    .or(game["image"]["square"].as_str())
                else {
                    continue;
                };
                for id in game["titles"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|title| title["id"].as_str())
                {
                    entries.insert(id.to_owned(), image.to_owned());
                }
            }
            *covers.write().await = entries;
            let mut state = data.write().await;
            let at = Utc::now().to_rfc3339();
            state["sources"]["covers"] = json!({"ok":true,"at":at});
            state["updatedAt"] = json!(at);
        }
        Err(error) => mark_source_error(&data, "covers", &error.to_string()).await,
    }
}

async fn publish_core_progress(app: &AppState, progress: &Result<Value>, history: &Result<Value>) {
    let Ok(progress) = progress else {
        return;
    };
    let now = Utc::now();
    let now_text = now.to_rfc3339();
    let date = now.format("%Y-%m-%d").to_string();
    let history_points = history
        .as_ref()
        .ok()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let persisted = app.persisted.read().await.clone();
    let groups = progress["libraries"]["groups"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let summaries: Vec<Value> = groups
        .iter()
        .map(|group| {
            json!({
                "name":group["name"],
                "label":group.get("label").unwrap_or(&group["name"]),
                "done":group["done"],
                "total":group["done"].as_i64().unwrap_or(0)+group["todo"].as_i64().unwrap_or(0)
            })
        })
        .collect();
    let snapshot_libraries: serde_json::Map<String, Value> = groups
        .iter()
        .map(|group| {
            (
                group["name"].as_str().unwrap_or_default().to_owned(),
                json!([
                    group["done"],
                    group["done"].as_i64().unwrap_or(0) + group["todo"].as_i64().unwrap_or(0)
                ]),
            )
        })
        .collect();
    let mut preview = persisted;
    if let Ok(points) = history {
        insert_events(
            &mut preview,
            history_events(points.as_array().map(Vec::as_slice).unwrap_or(&[])),
        );
    }
    let snapshots = preview["snapshots"].as_array_mut().unwrap();
    let previous_snapshot = snapshots.last().cloned();
    let snapshot = json!({"date":date,"libraries":snapshot_libraries});
    if snapshots.last().and_then(|item| item["date"].as_str()) == Some(date.as_str()) {
        *snapshots.last_mut().unwrap() = snapshot;
    } else {
        snapshots.push(snapshot);
    }
    if snapshots.len() > 35 {
        let remove = snapshots.len() - 35;
        snapshots.drain(0..remove);
    }
    let reference = snapshots
        .iter()
        .rev()
        .find(|item| item["date"].as_str().unwrap_or_default() < date.as_str())
        .cloned();
    if let Some(previous) = previous_snapshot {
        let mut changes = Vec::new();
        for group in &summaries {
            let name = group["name"].as_str().unwrap_or("");
            let current_total = group["total"].as_i64().unwrap_or(0);
            let current_done = group["done"].as_i64().unwrap_or(0);
            let Some(old) = previous["libraries"][name].as_array() else {
                changes.push(json!({"time":now_text,"kind":"discovery","title":format!("New library: {name}"),"detail":format!("{current_total} exports declared, {current_done} already implemented.")}));
                continue;
            };
            let old_done = old[0].as_i64().unwrap_or(0);
            let old_total = old[1].as_i64().unwrap_or(0);
            if current_total - old_total >= 5 {
                changes.push(json!({"time":now_text,"kind":"discovery","title":format!("{name}: +{} exports discovered",current_total-old_total)}));
            }
            if current_done - old_done >= 25 {
                changes.push(json!({"time":now_text,"kind":"library","title":format!("{name}: +{} functions implemented",current_done-old_done)}));
            }
            if current_total > 0 && current_done == current_total && old_done < old_total {
                changes.push(json!({"time":now_text,"kind":"complete","title":format!("{name} reached 100%"),"detail":format!("{current_total} of {current_total} functions.")}));
            }
        }
        insert_events(&mut preview, changes);
    }
    let mut all = summaries.clone();
    all.retain(|group| group["total"].as_i64().unwrap_or(0) > 0);
    all.sort_by_key(|group| -group["total"].as_i64().unwrap_or(0));
    let mut biggest = all.clone();
    biggest.truncate(10);
    let complete = summaries
        .iter()
        .filter(|group| group["total"].as_i64().unwrap_or(0) > 0 && group["done"] == group["total"])
        .count();
    let started = summaries
        .iter()
        .filter(|group| group["done"].as_i64().unwrap_or(0) > 0 && group["done"] != group["total"])
        .count();
    let untouched = summaries
        .iter()
        .filter(|group| group["done"].as_i64().unwrap_or(0) == 0)
        .count();
    let mut movers: Vec<Value> = reference
        .as_ref()
        .map(|reference| {
            summaries
                .iter()
                .filter_map(|group| {
                    let name = group["name"].as_str()?;
                    let before = reference["libraries"][name][0].as_i64().unwrap_or(0);
                    let gained = group["done"].as_i64().unwrap_or(0) - before;
                    (gained > 0).then(|| {
                        json!({
                            "name":group["name"], "label":group["label"], "done":group["done"],
                            "total":group["total"], "gained":gained
                        })
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    movers.sort_by_key(|item| -item["gained"].as_i64().unwrap_or(0));
    movers.truncate(8);
    let delta = |days: i64| {
        let target = (now - chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string();
        history_points
            .iter()
            .rev()
            .find(|item| item["date"].as_str().unwrap_or_default() <= target.as_str())
            .map(|item| {
                json!({
                    "done":progress["libraries"]["done"].as_i64().unwrap_or(0)-item["libraries"]["done"].as_i64().unwrap_or(0),
                    "total":progress["libraries"]["total"].as_i64().unwrap_or(0)-item["libraries"]["total"].as_i64().unwrap_or(0),
                    "percent":((progress["libraries"]["percent"].as_f64().unwrap_or(0.0)-item["libraries"]["percent"].as_f64().unwrap_or(0.0))*100.0).round()/100.0
                })
            })
            .unwrap_or(Value::Null)
    };
    let progress_view = json!({
        "done":progress["libraries"]["done"], "total":progress["libraries"]["total"],
        "percent":progress["libraries"]["percent"], "shadersDone":progress["shaders"]["done"],
        "shadersTotal":progress["shaders"]["total"], "shadersPercent":progress["shaders"]["percent"],
        "day":delta(1), "week":delta(7)
    });
    let libraries_view = json!({
        "count":summaries.len(), "complete":complete, "started":started, "untouched":untouched,
        "biggest":biggest, "all":all, "movers":movers,
        "moversSince":reference.as_ref().and_then(|item|item["date"].as_str())
    });
    let history_view = history
        .as_ref()
        .ok()
        .and_then(Value::as_array)
        .map(|entries| {
            json!(entries.iter().map(|entry|json!({
                "date":entry["date"],"commit":entry["commit"],"done":entry["libraries"]["done"],"total":entry["libraries"]["total"],
                "percent":entry["libraries"]["percent"],"shadersDone":entry["shaders"]["done"],"shadersTotal":entry["shaders"]["total"]
            })).collect::<Vec<_>>())
        });
    let mut state = app.data.write().await;
    state["progress"] = progress_view;
    state["libraries"] = libraries_view;
    state["events"] = preview["events"].clone();
    state["sources"]["progress"] = json!({"ok":true,"at":now_text});
    state["updatedAt"] = json!(now_text);
    if let Some(history_view) = history_view {
        state["history"] = history_view;
        state["sources"]["history"] = json!({"ok":true,"at":now_text});
    } else {
        state["sources"]["history"] =
            json!({"ok":false,"at":null,"error":"history source unavailable"});
    }
}

async fn collect(app: AppState) {
    let pages =
        env::var("ANYPS5_PAGES").unwrap_or_else(|_| "https://boykopovar.github.io/AnyPS5".into());
    let repo = env::var("ANYPS5_REPO").unwrap_or_else(|_| "boykopovar/AnyPS5".into());
    let token = env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty());
    let api_pulls = format!(
        "https://api.github.com/repos/{repo}/pulls?state=closed&sort=updated&direction=desc&per_page=100"
    );
    let api_open = format!(
        "https://api.github.com/repos/{repo}/pulls?state=open&sort=created&direction=desc&per_page=20"
    );
    let api_release = format!("https://api.github.com/repos/{repo}/releases/latest");
    loop {
        let progress_url = format!(
            "{}/progress.json?{}",
            pages.trim_end_matches('/'),
            Utc::now().timestamp_millis()
        );
        let history_url = format!(
            "{}/history.json?{}",
            pages.trim_end_matches('/'),
            Utc::now().timestamp_millis()
        );
        let api = format!("https://api.github.com/repos/{repo}");
        let search_open = format!(
            "https://api.github.com/search/issues?per_page=1&q={}",
            encode(&format!("repo:{repo} is:pr is:open"))
        );
        let seven_days = (Utc::now() - chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();
        let search_week = format!(
            "https://api.github.com/search/issues?per_page=1&q={}",
            encode(&format!(
                "repo:{repo} is:pr is:merged merged:>={seven_days}"
            ))
        );
        let day = (Utc::now() - chrono::Duration::days(1))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let search_day = format!(
            "https://api.github.com/search/issues?per_page=100&q={}",
            encode(&format!("repo:{repo} is:pr is:merged merged:>={day}"))
        );
        // Independent sources share a pooled client and are fetched concurrently.
        let (
            (progress, history),
            (repository, pulls, open_pulls, release, open_count, week_count, day_count),
        ) = tokio::join!(
            async {
                let (progress, history) = tokio::join!(
                    fetch_json(&app.client, &progress_url, None),
                    fetch_json(&app.client, &history_url, None)
                );
                publish_core_progress(&app, &progress, &history).await;
                (progress, history)
            },
            async {
                tokio::join!(
                    fetch_json(&app.client, &api, token.as_deref()),
                    fetch_json(&app.client, &api_pulls, token.as_deref()),
                    fetch_json(&app.client, &api_open, token.as_deref()),
                    fetch_json(&app.client, &api_release, token.as_deref()),
                    fetch_json(&app.client, &search_open, token.as_deref()),
                    fetch_json(&app.client, &search_week, token.as_deref()),
                    fetch_json(&app.client, &search_day, token.as_deref())
                )
            }
        );
        let mut merged_reports = std::collections::HashMap::<String, Value>::new();
        let mut open_report_values = std::collections::HashMap::<String, Value>::new();
        if let Ok(items) = &pulls {
            let existing = app.persisted.read().await["prReports"].clone();
            let mut pending = Vec::new();
            for pull in items
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| !p["merged_at"].is_null())
                .take(if token.is_some() { 14 } else { 2 })
            {
                let Some(number) = pull["number"].as_i64() else {
                    continue;
                };
                let key = number.to_string();
                let report = if let Some(cached) = existing.get(&key) {
                    cached.clone()
                } else {
                    let merged_at = chrono::DateTime::parse_from_rfc3339(
                        pull["merged_at"].as_str().unwrap_or("1970-01-01T00:00:00Z"),
                    )
                    .map(|date| date.timestamp())
                    .unwrap_or(0);
                    pending.push((key, number, merged_at));
                    continue;
                };
                merged_reports.insert(key, report);
            }
            let rate_remaining = GITHUB_RATE_REMAINING.load(Ordering::Relaxed);
            let fetched = if rate_remaining < 0 || rate_remaining >= 10 {
                fetch_reports_concurrently(
                    &app.client,
                    &repo,
                    pending
                        .iter()
                        .map(|(_, number, _)| *number)
                        .collect::<Vec<_>>(),
                    token.as_deref(),
                )
                .await
            } else {
                HashMap::new()
            };
            let now = Utc::now().timestamp();
            for (key, number, merged_at) in pending {
                if let Some(Ok(report)) = fetched.get(&number) {
                    if report.is_some() || now - merged_at > 120 {
                        merged_reports.insert(key, report.clone().unwrap_or(Value::Null));
                    }
                }
            }
        }
        if token.is_some() {
            if let Ok(items) = &open_pulls {
                let now = Utc::now().timestamp();
                let mut cache = app.persisted.read().await["openPrReports"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                let mut pending = Vec::new();
                let rate_remaining = GITHUB_RATE_REMAINING.load(Ordering::Relaxed);
                for pull in items
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|p| p["draft"] != true)
                    .take(6)
                {
                    let Some(number) = pull["number"].as_i64() else {
                        continue;
                    };
                    let key = number.to_string();
                    let entry = if cache
                        .get(&key)
                        .is_some_and(|e| now - e["at"].as_i64().unwrap_or(0) < 1200)
                    {
                        cache[&key].clone()
                    } else {
                        if let Some(entry) = cache.get(&key) {
                            open_report_values.insert(key.clone(), entry["report"].clone());
                        }
                        if rate_remaining < 0 || rate_remaining >= 50 {
                            pending.push(number);
                        }
                        continue;
                    };
                    open_report_values.insert(key.clone(), entry["report"].clone());
                }
                let fetched = fetch_reports_concurrently(
                    &app.client,
                    &repo,
                    pending.iter().copied().collect::<Vec<_>>(),
                    token.as_deref(),
                )
                .await;
                for number in pending {
                    let key = number.to_string();
                    if let Some(Ok(report)) = fetched.get(&number) {
                        let entry =
                            json!({"at":now,"report":report.clone().unwrap_or(Value::Null)});
                        open_report_values.insert(key.clone(), entry["report"].clone());
                        cache.insert(key, entry);
                    }
                }
                let mut persisted = app.persisted.write().await;
                persisted["openPrReports"] = Value::Object(cache);
            }
        }
        if !merged_reports.is_empty() {
            let mut persisted = app.persisted.write().await;
            let mut cache = persisted["prReports"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            for (number, report) in &merged_reports {
                cache.insert(number.clone(), report.clone());
            }
            while cache.len() > 400 {
                if let Some(key) = cache
                    .keys()
                    .min_by_key(|k| k.parse::<i64>().unwrap_or(0))
                    .cloned()
                {
                    cache.remove(&key);
                } else {
                    break;
                }
            }
            persisted["prReports"] = Value::Object(cache);
        }
        let mut persist_bytes = None;
        let search_ok = open_count.is_ok() && week_count.is_ok() && day_count.is_ok();
        {
            let mut state = app.data.write().await;
            let now = Utc::now().to_rfc3339();
            if let Ok(progress) = progress {
                let history_points = history
                    .as_ref()
                    .ok()
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let delta = |days: i64| {
                    let date = (Utc::now() - chrono::Duration::days(days))
                        .format("%Y-%m-%d")
                        .to_string();
                    history_points.iter().rev().find(|p|p["date"].as_str().unwrap_or_default()<=date.as_str()).map(|p|json!({
                        "done":progress["libraries"]["done"].as_i64().unwrap_or(0)-p["libraries"]["done"].as_i64().unwrap_or(0),
                        "total":progress["libraries"]["total"].as_i64().unwrap_or(0)-p["libraries"]["total"].as_i64().unwrap_or(0),
                        "percent":((progress["libraries"]["percent"].as_f64().unwrap_or(0.0)-p["libraries"]["percent"].as_f64().unwrap_or(0.0))*100.0).round()/100.0
                    })).unwrap_or(Value::Null)
                };
                state["progress"] = json!({
                    "done": progress["libraries"]["done"], "total": progress["libraries"]["total"],
                    "percent": progress["libraries"]["percent"], "shadersDone": progress["shaders"]["done"],
                    "shadersTotal": progress["shaders"]["total"], "shadersPercent": progress["shaders"]["percent"],
                    "day": delta(1), "week": delta(7)
                });
                let groups = progress["libraries"]["groups"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let summaries: Vec<Value> = groups.iter().map(|g| json!({
                    "name":g["name"], "label":g.get("label").unwrap_or(&g["name"]), "done":g["done"],
                    "total":g["done"].as_i64().unwrap_or(0)+g["todo"].as_i64().unwrap_or(0)
                })).collect();
                let date = Utc::now().format("%Y-%m-%d").to_string();
                let libraries: serde_json::Map<String, Value> = groups
                    .iter()
                    .map(|g| {
                        let name = g["name"].as_str().unwrap_or_default().to_owned();
                        (
                            name,
                            json!([
                                g["done"],
                                g["done"].as_i64().unwrap_or(0) + g["todo"].as_i64().unwrap_or(0)
                            ]),
                        )
                    })
                    .collect();
                let mut persisted = app.persisted.write().await;
                if let Ok(points) = history.as_ref() {
                    insert_events(
                        &mut persisted,
                        history_events(points.as_array().map(Vec::as_slice).unwrap_or(&[])),
                    );
                }
                let snapshots = persisted["snapshots"].as_array_mut().unwrap();
                let previous_snapshot = snapshots.last().cloned();
                let snapshot = json!({"date":date,"libraries":libraries});
                if snapshots.last().and_then(|x| x["date"].as_str()) == Some(date.as_str()) {
                    *snapshots.last_mut().unwrap() = snapshot;
                } else {
                    snapshots.push(snapshot);
                }
                if snapshots.len() > 35 {
                    let remove = snapshots.len() - 35;
                    snapshots.drain(0..remove);
                }
                let reference = snapshots
                    .iter()
                    .rev()
                    .find(|x| x["date"].as_str().unwrap_or_default() < date.as_str())
                    .cloned();
                if let Some(previous) = previous_snapshot {
                    let mut changes = Vec::new();
                    for group in &summaries {
                        let name = group["name"].as_str().unwrap_or("");
                        let current_total = group["total"].as_i64().unwrap_or(0);
                        let current_done = group["done"].as_i64().unwrap_or(0);
                        let old = previous["libraries"][name].as_array();
                        if old.is_none() {
                            changes.push(json!({"time":now,"kind":"discovery","title":format!("New library: {name}"),"detail":format!("{current_total} exports declared, {current_done} already implemented.")}));
                            continue;
                        }
                        let old = old.unwrap();
                        let old_done = old[0].as_i64().unwrap_or(0);
                        let old_total = old[1].as_i64().unwrap_or(0);
                        if current_total - old_total >= 5 {
                            changes.push(json!({"time":now,"kind":"discovery","title":format!("{name}: +{} exports discovered",current_total-old_total)}));
                        }
                        if current_done - old_done >= 25 {
                            changes.push(json!({"time":now,"kind":"library","title":format!("{name}: +{} functions implemented",current_done-old_done)}));
                        }
                        if current_total > 0
                            && current_done == current_total
                            && old_done < old_total
                        {
                            changes.push(json!({"time":now,"kind":"complete","title":format!("{name} reached 100%"),"detail":format!("{current_total} of {current_total} functions.")}));
                        }
                    }
                    insert_events(&mut persisted, changes);
                }
                let mut movers:Vec<Value>=reference.as_ref().map(|reference|summaries.iter().filter_map(|g|{
                    let name=g["name"].as_str()?;
                    let before=reference["libraries"][name][0].as_i64().unwrap_or(0);
                    let gained=g["done"].as_i64().unwrap_or(0)-before;
                    (gained>0).then(||json!({"name":g["name"],"label":g["label"],"done":g["done"],"total":g["total"],"gained":gained}))
                }).collect()).unwrap_or_default();
                movers.sort_by_key(|m| -m["gained"].as_i64().unwrap_or(0));
                movers.truncate(8);
                persist_bytes = serde_json::to_vec(&*persisted).ok();
                let current_events = persisted["events"].clone();
                drop(persisted);
                let mut all = summaries.clone();
                all.retain(|g| g["total"].as_i64().unwrap_or(0) > 0);
                all.sort_by_key(|g| -g["total"].as_i64().unwrap_or(0));
                let mut biggest = all.clone();
                biggest.truncate(10);
                let complete = summaries
                    .iter()
                    .filter(|g| g["total"].as_i64().unwrap_or(0) > 0 && g["done"] == g["total"])
                    .count();
                let started = summaries
                    .iter()
                    .filter(|g| g["done"].as_i64().unwrap_or(0) > 0 && g["done"] != g["total"])
                    .count();
                let untouched = summaries
                    .iter()
                    .filter(|g| g["done"].as_i64().unwrap_or(0) == 0)
                    .count();
                state["libraries"] = json!({"count":summaries.len(),"complete":complete,"started":started,"untouched":untouched,"biggest":biggest,"all":all,"movers":movers,"moversSince":reference.as_ref().and_then(|x|x["date"].as_str())});
                state["events"] = current_events;
                state["sources"]["progress"] = json!({"ok":true,"at":now});
            } else {
                state["sources"]["progress"] =
                    json!({"ok":false,"at":null,"error":"progress source unavailable"});
            }
            if let Ok(history) = history {
                state["history"]=json!(history.as_array().into_iter().flatten().map(|e|json!({
                    "date":e["date"],"commit":e["commit"],"done":e["libraries"]["done"],"total":e["libraries"]["total"],
                    "percent":e["libraries"]["percent"],"shadersDone":e["shaders"]["done"],"shadersTotal":e["shaders"]["total"]
                })).collect::<Vec<_>>());
                state["sources"]["history"] = json!({"ok":true,"at":now});
            } else {
                state["sources"]["history"] =
                    json!({"ok":false,"at":null,"error":"history source unavailable"});
            }
            if let Ok(repository) = repository {
                state["github"]["stars"] = repository["stargazers_count"].clone();
                state["github"]["authenticated"] = json!(token.is_some());
                let remaining = GITHUB_RATE_REMAINING.load(Ordering::Relaxed);
                state["github"]["rateRemaining"] = if remaining < 0 {
                    Value::Null
                } else {
                    json!(remaining)
                };
                state["sources"]["repo"] = json!({"ok":true,"at":now});
            } else {
                state["sources"]["repo"] =
                    json!({"ok":false,"at":null,"error":"GitHub repository unavailable"});
            }
            if let Ok(pulls) = pulls {
                let mut merged: Vec<Value> = pulls
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|p| !p["merged_at"].is_null())
                    .cloned()
                    .collect();
                merged.sort_by(|a, b| b["merged_at"].as_str().cmp(&a["merged_at"].as_str()));
                state["github"]["mergedPrs"]=json!(merged.iter().take(14).map(|p|json!({
                    "number":p["number"],"title":p["title"],"author":p["user"]["login"],"avatar":p["user"]["avatar_url"],
                    "mergedAt":p["merged_at"],"report":merged_reports.get(&p["number"].as_i64().unwrap_or(0).to_string()).filter(|v| !v.is_null()).cloned().unwrap_or(Value::Null)
                })).collect::<Vec<_>>());
                state["sources"]["github"] = json!({"ok":true,"at":now});
            } else {
                state["sources"]["github"] =
                    json!({"ok":false,"at":null,"error":"GitHub pull requests unavailable"});
            }
            if let Ok(pulls) = open_pulls {
                state["github"]["openedPrs"]=json!(pulls.as_array().into_iter().flatten().filter(|p|p["draft"]!=true).take(6).map(|p|json!({
                    "number":p["number"],"title":p["title"],"author":p["user"]["login"],"avatar":p["user"]["avatar_url"],
                    "createdAt":p["created_at"],"report":open_report_values.get(&p["number"].as_i64().unwrap_or(0).to_string()).filter(|v| !v.is_null()).cloned().unwrap_or(Value::Null)
                })).collect::<Vec<_>>());
            }
            if let Ok(count) = open_count {
                state["github"]["openPrs"] = count["total_count"].clone();
            }
            if let Ok(count) = week_count {
                state["github"]["merged7d"] = count["total_count"].clone();
            }
            if let Ok(count) = day_count {
                state["github"]["merged24h"] = count["total_count"].clone();
                let mut authors: std::collections::HashMap<String, Value> =
                    std::collections::HashMap::new();
                for pull in count["items"].as_array().into_iter().flatten() {
                    let login = pull["user"]["login"].as_str().unwrap_or("");
                    if login.ends_with("[bot]") {
                        continue;
                    }
                    let author = authors.entry(login.to_string()).or_insert_with(
                        || json!({"author":login,"avatar":pull["user"]["avatar_url"],"merged":0}),
                    );
                    author["merged"] = json!(author["merged"].as_i64().unwrap_or(0) + 1);
                }
                let mut authors: Vec<Value> = authors.into_values().collect();
                authors.sort_by_key(|a| -a["merged"].as_i64().unwrap_or(0));
                authors.truncate(8);
                state["github"]["topAuthors"] = json!(authors);
            }
            if search_ok {
                state["sources"]["search"] = json!({"ok":true,"at":now});
            } else {
                let at = state["sources"]["search"]["at"].clone();
                state["sources"]["search"] =
                    json!({"ok":false,"at":at,"error":"GitHub search unavailable"});
            }
            if let Ok(release) = release {
                state["github"]["release"] =
                    json!({"tag":release["tag_name"],"publishedAt":release["published_at"]});
                let tag = release["tag_name"].as_str().unwrap_or_default();
                let mut persisted = app.persisted.write().await;
                if !tag.is_empty()
                    && !persisted["releaseTag"].is_null()
                    && persisted["releaseTag"].as_str() != Some(tag)
                {
                    insert_events(
                        &mut persisted,
                        vec![
                            json!({"time":release["published_at"],"kind":"release","title":format!("New release {tag} published")}),
                        ],
                    );
                }
                if !tag.is_empty() {
                    persisted["releaseTag"] = json!(tag);
                }
            }
            state["youtube"]["shorts"] = json!(
                env::var("YOUTUBE_REFRESH_TOKEN")
                    .map(|x| !x.is_empty())
                    .unwrap_or(false)
            );
            state["updatedAt"] = json!(now);
        }
        if let Some(bytes) = persist_bytes {
            let temporary = app.data_file.with_extension("json.tmp");
            if fs::write(&temporary, bytes).await.is_ok() {
                let _ = fs::rename(temporary, &app.data_file).await;
            }
        }
        // Authenticated activity cards refresh at upstream's two minute cadence;
        // the remaining GitHub API work shares this loop and stays within quota.
        tokio::time::sleep(Duration::from_secs(if token.is_some() { 120 } else { 600 })).await;
    }
}

async fn save_persisted(app: &AppState) {
    let bytes = {
        let persisted = app.persisted.read().await;
        serde_json::to_vec(&*persisted)
    };
    if let Ok(bytes) = bytes {
        let temporary = app.data_file.with_extension("json.tmp");
        if fs::write(&temporary, bytes).await.is_ok() {
            let _ = fs::rename(temporary, &app.data_file).await;
        }
    }
}

async fn answer_chat_messages(
    app: &AppState,
    messages: Vec<chat::StreamMessage>,
    asked: &mut HashMap<String, i64>,
    burst_until: &mut i64,
) {
    let now = Utc::now().timestamp_millis();
    asked.retain(|_, at| now - *at < 30_000);
    let threads = app.threads.read().await.clone();
    let (libraries, progress, top) = {
        let data = app.data.read().await;
        (
            data["libraries"]["all"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            data["progress"].clone(),
            data["github"]["topAuthors"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        )
    };
    let mut answers = Vec::new();
    for message in messages {
        if answers.len() >= 3 || message.kind != 1 || asked.contains_key(&message.author_id) {
            continue;
        }
        let Some(answer) = chat::command_answer(&message, &threads, &libraries, &progress, &top)
        else {
            continue;
        };
        asked.insert(message.author_id, now);
        let delay = DateTime::parse_from_rfc3339(&message.published_at)
            .map(|at| (now - at.timestamp_millis()) as f64 / 1000.0)
            .unwrap_or_default();
        println!(
            "[chat] answered {} {:.1} s after it was sent",
            message.text.split_whitespace().next().unwrap_or("!command"),
            delay
        );
        answers.push(answer);
    }
    if answers.is_empty() {
        return;
    }
    *burst_until = now
        + env::var("CHAT_BURST_MINUTES")
            .ok()
            .and_then(|x| x.parse::<i64>().ok())
            .unwrap_or(3)
            .max(0)
            * 60_000;
    let mut state = app.data.write().await;
    let list = state["chat"]["answers"].as_array_mut().unwrap();
    list.extend(answers);
    if list.len() > 10 {
        list.drain(0..list.len() - 10);
    }
}

async fn chat_loop(app: AppState) {
    let Ok(key) = env::var("YOUTUBE_API_KEY") else {
        return;
    };
    let daily_units = env::var("CHAT_DAILY_UNITS")
        .ok()
        .and_then(|x| x.parse::<i64>().ok())
        .unwrap_or(8000);
    let idle = env::var("CHAT_POLL_SECONDS")
        .ok()
        .and_then(|x| x.parse::<u64>().ok())
        .unwrap_or(75)
        .max(30);
    let mut page_token: Option<String> = None;
    let mut chat_id: Option<String> = None;
    let mut burst_until = 0_i64;
    let mut asked = HashMap::<String, i64>::new();
    loop {
        let live_id = app.live_chat_id.read().await.clone();
        if live_id != chat_id {
            chat_id = live_id;
            page_token = None;
            asked.clear();
            burst_until = 0;
        }
        let Some(live_id) = chat_id.clone() else {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        };
        let day = chat::quota_day(Utc::now().timestamp_millis());
        let units = {
            let mut persisted = app.persisted.write().await;
            if persisted["chatUnits"]["day"].as_str() != Some(&day) {
                persisted["chatUnits"] = json!({"day":day,"units":0});
            }
            let units = persisted["chatUnits"]["units"].as_i64().unwrap_or(0);
            if units + 5 <= daily_units {
                persisted["chatUnits"]["units"] = json!(units + 5);
            }
            units
        };
        if units + 5 > daily_units {
            tokio::time::sleep(Duration::from_secs(300)).await;
            continue;
        }
        save_persisted(&app).await;
        match chat::open_stream(&key, &live_id, page_token.as_deref()).await {
            Ok((mut stream, _channel)) => {
                let mut skip_history = page_token.is_none();
                loop {
                    match stream.message().await {
                        Ok(Some(response)) => {
                            if let Some(token) = response.next_page_token.clone() {
                                page_token = Some(token);
                            }
                            let messages = chat::messages(response);
                            if skip_history {
                                skip_history = false;
                                continue;
                            }
                            answer_chat_messages(&app, messages, &mut asked, &mut burst_until)
                                .await;
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let mut state = app.data.write().await;
                            state["sources"]["chat"] = json!({"ok":false,"at":null,"error":format!("YouTube chat stream {error}")});
                            burst_until = 0;
                            break;
                        }
                    }
                }
                let mut state = app.data.write().await;
                state["sources"]["chat"] = json!({"ok":true,"at":Utc::now().to_rfc3339()});
            }
            Err(error) => {
                app.data.write().await["sources"]["chat"] =
                    json!({"ok":false,"at":null,"error":format!("YouTube chat stream {error:#}")});
                burst_until = 0;
            }
        }
        let now = Utc::now().timestamp_millis();
        let delay = chat::next_chat_delay(now, burst_until, daily_units - units, idle);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
}

async fn chat_reminder(app: AppState) {
    let (Ok(client_id), Ok(secret), Ok(refresh)) = (
        env::var("YOUTUBE_CLIENT_ID"),
        env::var("YOUTUBE_CLIENT_SECRET"),
        env::var("YOUTUBE_REFRESH_TOKEN"),
    ) else {
        return;
    };
    let Some(live_id) = app.live_chat_id.read().await.clone() else {
        return;
    };
    let minutes = env::var("CHAT_REMINDER_MINUTES")
        .ok()
        .and_then(|x| x.parse::<i64>().ok())
        .unwrap_or(60)
        .max(0);
    if minutes == 0 {
        return;
    }
    let now = Utc::now().timestamp_millis();
    {
        let mut persisted = app.persisted.write().await;
        let at = persisted["chatReminderAt"].as_i64();
        if at.is_none() {
            persisted["chatReminderAt"] = json!(now);
            drop(persisted);
            save_persisted(&app).await;
            return;
        }
        if now - at.unwrap_or(now) < minutes * 60_000 {
            return;
        }
        persisted["chatReminderAt"] = json!(now);
    }
    save_persisted(&app).await;
    let token_url = env::var("GOOGLE_TOKEN_URL")
        .unwrap_or_else(|_| "https://oauth2.googleapis.com/token".into());
    let tokens = match app
        .client
        .post(token_url)
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v["access_token"].as_str().map(str::to_owned)),
        _ => None,
    };
    let Some(token) = tokens else {
        app.data.write().await["sources"]["chatReminder"] =
            json!({"ok":false,"at":null,"error":"Google token refresh failed"});
        return;
    };
    let response = app.client.post("https://www.googleapis.com/youtube/v3/liveChat/messages?part=snippet").bearer_auth(token).json(&json!({"snippet":{"liveChatId":live_id,"type":"textMessageEvent","textMessageDetails":{"messageText":chat::CHAT_REMINDER}}})).send().await;
    let mut state = app.data.write().await;
    state["sources"]["chatReminder"] = match response {
        Ok(r) if r.status().is_success() => json!({"ok":true,"at":Utc::now().to_rfc3339()}),
        Ok(r) => json!({"ok":false,"at":null,"error":format!("YouTube chat post {}",r.status())}),
        Err(e) => json!({"ok":false,"at":null,"error":e.to_string()}),
    };
}

fn encode(input: &str) -> String {
    input
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn formatted(value: i64) -> String {
    let raw = value.abs().to_string();
    let mut result = String::new();
    for (index, ch) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index) % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    if value < 0 {
        format!("−{result}")
    } else {
        result
    }
}

fn history_events(history: &[Value]) -> Vec<Value> {
    let mut events = Vec::new();
    for pair in history.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        let done = current["libraries"]["done"].as_i64().unwrap_or(0);
        let total = current["libraries"]["total"].as_i64().unwrap_or(0);
        let old_done = previous["libraries"]["done"].as_i64().unwrap_or(0);
        let old_total = previous["libraries"]["total"].as_i64().unwrap_or(0);
        let gained = done - old_done;
        let discovered = total - old_total;
        let date = current["date"].as_str().unwrap_or_default();
        let time = format!("{date}T12:00:00Z");
        if discovered >= 300 {
            events.push(json!({"time":time,"kind":"discovery","title":format!("{} PS5 exports declared",formatted(discovered)),"detail":format!("The tracked total grew from {} to {} functions.",formatted(old_total),formatted(total))}));
        }
        if gained >= 150 {
            events.push(json!({"time":time,"kind":"jump","title":format!("{} functions implemented in a single day",formatted(gained)),"detail":format!("{} → {} functions done.",formatted(old_done),formatted(done))}));
        }
        let pct = current["libraries"]["percent"].as_f64().unwrap_or(0.0);
        let old_pct = previous["libraries"]["percent"].as_f64().unwrap_or(0.0);
        let movement = pct - old_pct;
        if movement.abs() >= 1.0 {
            let widened = movement < 0.0 && discovered >= 300;
            let signed = if movement >= 0.0 {
                format!("+{movement:.2}")
            } else {
                format!("−{:.2}", movement.abs())
            };
            let title = if widened {
                format!("Completion rate {signed} pts (total widened)")
            } else {
                format!("Completion rate {signed} pts (now {pct:.2}%)")
            };
            let detail = if widened {
                "Mechanical drop: the total widened, no function was lost.".to_owned()
            } else {
                format!("From {old_pct:.2}% to {pct:.2}%.")
            };
            events.push(json!({"time":time,"kind":"percent","title":title,"detail":detail}));
        }
        for mark in (old_done / 1000 + 1)..=(done / 1000) {
            events.push(json!({"time":time,"kind":"milestone","title":format!("{} implemented functions reached",formatted(mark*1000))}));
        }
        let shaders_done = current["shaders"]["done"].as_i64().unwrap_or(0);
        let shaders_total = current["shaders"]["total"].as_i64().unwrap_or(0);
        let previous_shaders = previous["shaders"]["done"].as_i64().unwrap_or(0);
        let previous_shader_total = previous["shaders"]["total"].as_i64().unwrap_or(0);
        if shaders_total > 0
            && shaders_done == shaders_total
            && previous_shaders < previous_shader_total
        {
            events.push(json!({"time":time,"kind":"milestone","title":"GPU shaders: 100% of RDNA instructions decoded"}));
        }
    }
    events
}

fn insert_events(persisted: &mut Value, events: Vec<Value>) {
    let list = persisted["events"].as_array_mut().unwrap();
    for event in events {
        let key = format!(
            "{}|{}|{}",
            event["kind"].as_str().unwrap_or(""),
            event["title"].as_str().unwrap_or(""),
            event["time"].as_str().unwrap_or("").get(..10).unwrap_or("")
        );
        let exists = list.iter().any(|e| {
            format!(
                "{}|{}|{}",
                e["kind"].as_str().unwrap_or(""),
                e["title"].as_str().unwrap_or(""),
                e["time"].as_str().unwrap_or("").get(..10).unwrap_or("")
            ) == key
        });
        if !exists {
            list.push(event);
        }
    }
    list.sort_by(|a, b| b["time"].as_str().cmp(&a["time"].as_str()));
    if list.len() > 200 {
        list.truncate(200);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let started = Utc::now().to_rfc3339();
    let data_file =
        PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "./data".into())).join("state.json");
    if let Some(parent) = data_file.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut persisted = json!({"snapshots":[],"events":[],"prReports":{},"openPrReports":{},"releaseTag":null,"chatUnits":null,"chatReminderAt":null});
    if let Ok(bytes) = fs::read(&data_file).await {
        if let Ok(saved) = serde_json::from_slice::<Value>(&bytes) {
            for key in [
                "snapshots",
                "events",
                "prReports",
                "openPrReports",
                "releaseTag",
                "chatUnits",
                "chatReminderAt",
            ] {
                if let Some(v) = saved.get(key) {
                    persisted[key] = v.clone();
                }
            }
        }
    }
    let data = Arc::new(RwLock::new(json!({
        "updatedAt": started, "startedAt": started, "sources": {}, "progress": null, "history": [],
        "libraries": null,
        "github": {"openPrs":null,"merged24h":null,"merged7d":null,"mergedPrs":[],"openedPrs":[],"topAuthors":[],"release":null,"rateRemaining":null,"authenticated":env::var("GITHUB_TOKEN").is_ok_and(|x|!x.is_empty())},
        "games": null, "events": [],
        "youtube": {"likes": null, "viewers": null, "shorts": env::var("YOUTUBE_REFRESH_TOKEN").is_ok_and(|x| !x.is_empty())},
        "chat": {"enabled": env::var("CHAT_COMMANDS").as_deref() == Ok("1") && env::var("YOUTUBE_API_KEY").is_ok_and(|x| !x.is_empty()) && env::var("YOUTUBE_VIDEO_ID").is_ok_and(|x| !x.is_empty()), "answers": []}
    })));
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("user-agent", "anyps5-live-dashboard".parse()?);
    headers.insert("accept", "application/vnd.github+json".parse()?);
    headers.insert("x-github-api-version", "2022-11-28".parse()?);
    let app = AppState {
        data,
        persisted: Arc::new(RwLock::new(persisted)),
        data_file,
        client: Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(60))
            .build()?,
        threads: Arc::new(RwLock::new(Arc::new(Vec::new()))),
        covers: Arc::new(RwLock::new(HashMap::new())),
        live_chat_id: Arc::new(RwLock::new(None)),
    };
    tokio::spawn(collect(app.clone()));
    tokio::spawn({
        let client = app.client.clone();
        let data = app.data.clone();
        let live_chat_id = app.live_chat_id.clone();
        async move { youtube_stats(&client, data, live_chat_id).await }
    });
    tokio::spawn({
        let client = app.client.clone();
        let data = app.data.clone();
        let covers = app.covers.clone();
        async move {
            loop {
                collect_covers(&client, data.clone(), covers.clone()).await;
                tokio::time::sleep(Duration::from_secs(180)).await;
            }
        }
    });
    tokio::spawn({
        let client = app.client.clone();
        let data = app.data.clone();
        let threads = app.threads.clone();
        let covers = app.covers.clone();
        async move {
            loop {
                collect_games(&client, data.clone(), threads.clone(), covers.clone()).await;
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
        }
    });
    if env::var("CHAT_COMMANDS").as_deref() == Ok("1") {
        tokio::spawn(chat_loop(app.clone()));
        if env::var("CHAT_REMINDER_MINUTES")
            .ok()
            .and_then(|x| x.parse::<u64>().ok())
            .unwrap_or(60)
            > 0
        {
            tokio::spawn({
                let app = app.clone();
                async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(300)).await;
                        chat_reminder(app.clone()).await;
                    }
                }
            });
        }
    }
    let public = env::var("PUBLIC_DIR").unwrap_or_else(|_| "./public".into());
    let router = Router::new()
        .route("/api/state", get(state))
        .route("/api/chat", get(chat_state))
        .route("/api/short-meta", get(short_meta))
        .route("/healthz", get(health))
        .fallback_service(ServeDir::new(public))
        .layer(middleware::from_fn(cache_policy))
        .with_state(app);
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("AnyPS5 Rust dashboard on http://localhost:{port}");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, router).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, apply_cache_policy, game_stage, publish_core_progress, report_counts,
        serialize_state,
    };
    use axum::{body::Body, http::StatusCode, response::Response};
    use chrono::Utc;
    use serde_json::{Value, json};
    use std::{collections::HashMap, path::PathBuf, sync::Arc};
    use tokio::sync::RwLock;

    #[test]
    fn game_stage_rules_reuse_match_behavior() {
        for (answer, expected) in [
            ("Does not convert", Some(0)),
            ("Before the first frame", Some(1)),
            ("Stops before first frame", None),
            ("Logos", Some(2)),
            ("Menu", Some(3)),
            ("Playable", Some(4)),
            ("- [x] In game", None),
        ] {
            let body = format!("## Furthest point\n\n{answer}");
            assert_eq!(game_stage(&body), expected, "{answer}");
        }
    }

    #[test]
    fn report_count_patterns_preserve_signed_and_spaced_numbers() {
        let counts = report_counts("+1 234 implemented functions, −56 declared function");
        assert_eq!(counts["implemented"], 1234);
        assert_eq!(counts["declared"], -56);
        assert_eq!(report_counts("no counts here")["implemented"], 0);
    }

    #[test]
    fn state_serialization_matches_the_owned_json_response() {
        let data = json!({"a": 1, "nested": {"ready": true}, "serverTime": "stale"});
        let serialized: Value =
            serde_json::from_slice(&serialize_state(&data, "current").unwrap()).unwrap();
        let mut expected = data;
        expected["serverTime"] = json!("current");
        assert_eq!(serialized, expected);
    }

    #[test]
    fn cache_headers_match_the_original_dynamic_and_static_routes() {
        let mut api = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();
        apply_cache_policy("/api/state", &mut api);
        assert_eq!(api.headers()["cache-control"], "no-store");
        assert_eq!(
            api.headers()["content-type"],
            "application/json; charset=utf-8"
        );

        let mut asset = Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();
        apply_cache_policy("/styles.css", &mut asset);
        assert_eq!(asset.headers()["cache-control"], "no-cache");

        let mut html = Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();
        apply_cache_policy("/index.html", &mut html);
        assert_eq!(html.headers()["content-type"], "text/html; charset=utf-8");

        let mut health = Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();
        apply_cache_policy("/healthz", &mut health);
        assert!(!health.headers().contains_key("cache-control"));
    }

    #[tokio::test]
    async fn readiness_snapshot_contains_the_complete_progress_contract() {
        let today = Utc::now().date_naive();
        let yesterday = (today - chrono::Days::new(1)).to_string();
        let week_ago = (today - chrono::Days::new(7)).to_string();
        let data = Arc::new(RwLock::new(json!({
            "progress": null, "libraries": null, "history": [], "events": [], "sources": {}
        })));
        let app = AppState {
            data: data.clone(),
            persisted: Arc::new(RwLock::new(json!({
                "snapshots":[{"date":yesterday,"libraries":{"A":[8,14],"B":[18,20]}}],
                "events":[],"prReports":{},"openPrReports":{},"releaseTag":null,
                "chatUnits":null,"chatReminderAt":null
            }))),
            data_file: PathBuf::new(),
            client: reqwest::Client::new(),
            threads: Arc::new(RwLock::new(Arc::new(Vec::new()))),
            covers: Arc::new(RwLock::new(HashMap::new())),
            live_chat_id: Arc::new(RwLock::new(None)),
        };
        let progress: super::Result<Value> = Ok(json!({
            "libraries":{"done":100,"total":200,"percent":50.0,"groups":[
                {"name":"A","label":"Alpha","done":10,"todo":5},
                {"name":"B","label":"Beta","done":20,"todo":0}
            ]},
            "shaders":{"done":3,"total":6,"percent":50.0}
        }));
        let history: super::Result<Value> = Ok(json!([
            {"date":week_ago,"commit":"old","libraries":{"done":70,"total":150,"percent":46.67},"shaders":{"done":2,"total":6}},
            {"date":yesterday,"commit":"recent","libraries":{"done":90,"total":180,"percent":50.0},"shaders":{"done":3,"total":6}}
        ]));

        publish_core_progress(&app, &progress, &history).await;
        let state = data.read().await;
        assert_eq!(state["progress"]["day"]["done"], 10);
        assert_eq!(state["progress"]["week"]["done"], 30);
        assert_eq!(state["libraries"]["count"], 2);
        assert_eq!(state["libraries"]["all"][0]["name"], "B");
        assert_eq!(state["libraries"]["movers"].as_array().unwrap().len(), 2);
        assert_eq!(state["libraries"]["movers"][0]["label"], "Alpha");
        assert!(state["libraries"]["movers"][0].get("todo").is_none());
        assert_eq!(state["sources"]["progress"]["ok"], true);
        assert_eq!(state["sources"]["history"]["ok"], true);
    }
}
