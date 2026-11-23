//! Leonida (GTA VI, Xbox Series, recompiled to run natively on PC): the repository the stream follows now.
//! Every minute: stars, forks, PRs merged today and in all, open PRs, the latest merges; every 5 minutes the
//! contributors and the README's status table. Served as `leonida` in /api/state for leonida.html.
use crate::{AppState, fetch_json, mark_source_error, save_persisted};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{Value, json};
use std::{env, sync::LazyLock, time::Duration};

const REFRESH_SECONDS: u64 = 60;
const SLOW_EVERY: u64 = 5;
// One star count every 5 minutes, three days of them.
const HISTORY_STEP_MS: i64 = 5 * 60_000;
const HISTORY_POINTS: usize = 3 * 24 * 12;

static LINK_LAST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[?&]page=(\d+)>; rel="last""#).unwrap());
static MD_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap());
static PARENS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\([^()]*\)").unwrap());
// Where a README row's first statement ends: the rows run to whole paragraphs.
static CLAUSE_END_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\. |\.$|: |; | — | in the |, with ").unwrap());
// A merge commit names its pull request: "Merge pull request #N" (GitHub) or "origin/pr/N" (merged locally).
static MERGED_PR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:pull request #|origin/pr/)(\d+)").unwrap());
static HEADLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)## Project status\s*\n+\*\*([^*]+)\*\*").unwrap());

pub fn repo() -> String {
    env::var("LEONIDA_REPO").unwrap_or_else(|_| "ProjectLeonida/Leonida".into())
}

/// The README's "Project status": its bold headline and one short line per table row.
pub fn parse_status(readme: &str) -> Value {
    let headline = HEADLINE_RE
        .captures(readme)
        .map(|c| c[1].trim().to_owned())
        .unwrap_or_default();
    let section = readme
        .split("## Project status")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .unwrap_or("");
    let rows: Vec<Value> = section
        .lines()
        .filter(|line| line.starts_with('|') && !line.contains("---"))
        .filter_map(|line| {
            let cells: Vec<&str> = line
                .trim_matches('|')
                .splitn(2, '|')
                .map(str::trim)
                .collect();
            let (what, status) = (cells.first()?, cells.get(1)?);
            if *what == "What" {
                return None;
            }
            let mark = if status.starts_with('✅') {
                "done"
            } else if status.starts_with('❌') {
                "no"
            } else {
                "partial"
            };
            Some(json!({"what": tidy(what), "mark": mark, "note": first_clause(status)}))
        })
        .collect();
    // The row that says whether the game boots: the page, the chat and the big moment follow it.
    let boot = rows
        .iter()
        .find(|row| {
            row["what"]
                .as_str()
                .is_some_and(|w| w.to_lowercase().contains("boot"))
        })
        .map(|row| json!({"mark": row["mark"], "note": row["note"]}))
        .unwrap_or(Value::Null);
    json!({"headline": headline, "rows": rows, "boot": boot})
}

// Links, bold, code marks and asides in parentheses out: what is left reads on a stream.
fn tidy(text: &str) -> String {
    let text = MD_LINK_RE
        .replace_all(text, "$1")
        .replace("**", "")
        .replace('`', "");
    PARENS_RE.replace_all(&text, "").trim().to_owned()
}

fn first_clause(status: &str) -> String {
    let text = tidy(status);
    let text = text.trim_start_matches(['✅', '❌', '⚠', '\u{fe0f}', '?', ' ']);
    let end = CLAUSE_END_RE.find(text).map_or(text.len(), |m| m.start());
    text[..end].trim().chars().take(90).collect()
}

/// While the README still says the game does not boot, public/leonida-boot.json can say where the boot stands
/// (from the project's own boot docs): it replaces the boot row, its note and the headline. Once the README's
/// boot row changes, the README wins.
pub fn apply_boot_override(status: &Value, overlay: &Value) -> Value {
    let mut status = status.clone();
    let Some(mark) = overlay["mark"].as_str() else {
        return status;
    };
    if status["boot"]["mark"].as_str().unwrap_or("no") != "no" {
        return status;
    }
    let note = overlay["note"].clone();
    if let Some(rows) = status["rows"].as_array_mut() {
        for row in rows.iter_mut() {
            if row["what"]
                .as_str()
                .is_some_and(|w| w.to_lowercase().contains("boot"))
            {
                row["mark"] = json!(mark);
                row["note"] = note.clone();
            }
        }
    }
    status["boot"] = json!({"mark": mark, "note": note});
    if let Some(headline) = overlay["headline"].as_str() {
        status["headline"] = json!(headline);
    }
    status
}

async fn boot_override() -> Value {
    let public = env::var("PUBLIC_DIR").unwrap_or_else(|_| "./public".into());
    tokio::fs::read_to_string(std::path::PathBuf::from(public).join("leonida-boot.json"))
        .await
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

async fn count(app: &AppState, token: Option<&str>, query: &str) -> Option<i64> {
    let url = format!(
        "https://api.github.com/search/issues?per_page=1&q={}",
        crate::encode(&format!("repo:{} {query}", repo()))
    );
    fetch_json(&app.client, &url, token).await.ok()?["total_count"].as_i64()
}

// Pull requests merged locally ("Merge remote-tracking branch 'origin/pr/N'") and closed: GitHub does not count
// them as merged, so their merge commits are counted instead.
async fn count_local_merges(
    app: &AppState,
    token: Option<&str>,
    since: Option<&str>,
) -> Option<i64> {
    let date = since
        .map(|d| format!(" committer-date:>={d}"))
        .unwrap_or_default();
    let url = format!(
        "https://api.github.com/search/commits?per_page=1&q={}",
        crate::encode(&format!("repo:{} merge:true \"origin/pr\"{date}", repo()))
    );
    fetch_json(&app.client, &url, token).await.ok()?["total_count"].as_i64()
}

/// The pull requests that landed last, newest first, from the merge commits on the default branch: whether
/// GitHub merged them or they were merged locally and closed. Titles and authors come from the recently closed
/// pull requests, or from the pull request itself.
async fn latest_landed(app: &AppState, token: Option<&str>, closed: &[Value]) -> Vec<Value> {
    let commits_url = format!(
        "https://api.github.com/repos/{}/commits?per_page=40",
        repo()
    );
    let commits = fetch_json(&app.client, &commits_url, token)
        .await
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let mut landed: Vec<(i64, String)> = Vec::new();
    for commit in &commits {
        let message = commit["commit"]["message"].as_str().unwrap_or("");
        let Some(number) = MERGED_PR_RE
            .captures(message.lines().next().unwrap_or(""))
            .and_then(|c| c[1].parse::<i64>().ok())
        else {
            continue;
        };
        if landed.iter().all(|(n, _)| *n != number) {
            let at = commit["commit"]["committer"]["date"]
                .as_str()
                .unwrap_or("")
                .to_owned();
            landed.push((number, at));
        }
        if landed.len() == 8 {
            break;
        }
    }
    let mut latest = Vec::new();
    for (number, at) in landed {
        let known = closed
            .iter()
            .find(|p| p["number"].as_i64() == Some(number))
            .cloned();
        let pull = match known {
            Some(pull) => pull,
            None => {
                let url = format!("https://api.github.com/repos/{}/pulls/{number}", repo());
                match fetch_json(&app.client, &url, token).await {
                    Ok(pull) => pull,
                    Err(_) => continue,
                }
            }
        };
        latest.push(json!({"number": number, "title": pull["title"], "author": pull["user"]["login"], "avatar": pull["user"]["avatar_url"], "mergedAt": at}));
    }
    latest
}

async fn contributors(app: &AppState, token: Option<&str>) -> Option<i64> {
    let mut request = app.client.get(format!(
        "https://api.github.com/repos/{}/contributors?per_page=1&anon=1",
        repo()
    ));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.ok()?.error_for_status().ok()?;
    let link = response.headers().get("link")?.to_str().ok()?.to_owned();
    LINK_LAST_RE.captures(&link)?[1].parse().ok()
}

async fn readme(app: &AppState, token: Option<&str>) -> Option<String> {
    let mut request = app
        .client
        .get(format!("https://api.github.com/repos/{}/readme", repo()))
        .header("accept", "application/vnd.github.raw+json");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()
}

// The star counts kept every 5 minutes, for the growth chart and the gains.
async fn record_stars(app: &AppState, now: i64, stars: i64) -> Vec<Value> {
    let history = {
        let mut persisted = app.persisted.write().await;
        let mut history = persisted["leonidaStars"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let last = history.last().and_then(|p| p[0].as_i64()).unwrap_or(0);
        if now - last >= HISTORY_STEP_MS {
            history.push(json!([now, stars]));
            if history.len() > HISTORY_POINTS {
                history.drain(0..history.len() - HISTORY_POINTS);
            }
            persisted["leonidaStars"] = json!(history);
        }
        history
    };
    save_persisted(app).await;
    history
}

fn gain_since(history: &[Value], now: i64, ago_ms: i64, stars: i64) -> Option<i64> {
    let past = history
        .iter()
        .filter(|p| p[0].as_i64().is_some_and(|at| at <= now - ago_ms))
        .next_back()?;
    Some(stars - past[1].as_i64()?)
}

// The last known feed, kept when a refresh brings none.
async fn state_latest(app: &AppState) -> Vec<Value> {
    app.data.read().await["leonida"]["latestMerged"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

pub async fn collect_loop(app: AppState) {
    let token = env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty());
    let token = token.as_deref();
    let mut turn = 0_u64;
    let mut slow = json!({"contributors": null, "status": null});
    loop {
        let repo_url = format!("https://api.github.com/repos/{}", repo());
        let pulls_url = format!(
            "https://api.github.com/repos/{}/pulls?state=closed&sort=updated&direction=desc&per_page=40",
            repo()
        );
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let merged_today_query = format!("is:pr is:merged merged:>={today}");
        let (meta, pulls, merged, merged_today, open, local, local_today) = tokio::join!(
            fetch_json(&app.client, &repo_url, token),
            fetch_json(&app.client, &pulls_url, token),
            count(&app, token, "is:pr is:merged"),
            count(&app, token, &merged_today_query),
            count(&app, token, "is:pr is:open"),
            count_local_merges(&app, token, None),
            count_local_merges(&app, token, Some(&today)),
        );
        let merged = merged.map(|n| n + local.unwrap_or(0));
        let merged_today = merged_today.map(|n| n + local_today.unwrap_or(0));
        let closed = pulls
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let latest = latest_landed(&app, token, &closed).await;
        if turn % SLOW_EVERY == 0 {
            let (people, text) = tokio::join!(contributors(&app, token), readme(&app, token));
            if let Some(people) = people {
                slow["contributors"] = json!(people);
            }
            if let Some(text) = text {
                slow["status"] = parse_status(&text);
            }
        }
        turn += 1;
        match meta {
            Ok(meta) => {
                let now = Utc::now().timestamp_millis();
                let stars = meta["stargazers_count"].as_i64().unwrap_or(0);
                let history = record_stars(&app, now, stars).await;
                let created = meta["created_at"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.timestamp_millis());
                let hours = created.map(|c| ((now - c) as f64 / 3_600_000.0).max(1.0));
                let latest = if latest.is_empty() {
                    state_latest(&app).await
                } else {
                    latest
                };
                let status = apply_boot_override(&slow["status"], &boot_override().await);
                // When the game first stops "not booting", kept so the page can celebrate it after a restart too.
                let boot_since = {
                    let mut persisted = app.persisted.write().await;
                    let booting = status["boot"]["mark"].as_str().is_some_and(|m| m != "no");
                    if booting && persisted["leonidaBootSince"].is_null() {
                        persisted["leonidaBootSince"] = json!(Utc::now().to_rfc3339());
                    } else if !booting && !status.is_null() {
                        persisted["leonidaBootSince"] = Value::Null;
                    }
                    persisted["leonidaBootSince"].clone()
                };
                let mut state = app.data.write().await;
                let previous = state["leonida"].clone();
                let keep = |key: &str, fresh: Option<i64>| {
                    fresh.map_or(previous[key].clone(), |v| json!(v))
                };
                state["leonida"] = json!({
                    "repo": repo(),
                    "stars": stars,
                    "forks": meta["forks_count"],
                    "watchers": meta["subscribers_count"],
                    "createdAt": meta["created_at"],
                    "pushedAt": meta["pushed_at"],
                    "starsPerHour": hours.map(|h| (stars as f64 / h).round()),
                    "starsLastHour": gain_since(&history, now, 3_600_000, stars),
                    "starsToday": gain_since(&history, now, 86_400_000, stars),
                    "starHistory": history,
                    "mergedTotal": keep("mergedTotal", merged),
                    "mergedToday": keep("mergedToday", merged_today),
                    "openPrs": keep("openPrs", open),
                    "contributors": slow["contributors"],
                    "status": status,
                    "bootSince": boot_since,
                    "latestMerged": latest,
                });
                state["sources"]["leonida"] = json!({"ok":true,"at":Utc::now().to_rfc3339()});
            }
            Err(error) => mark_source_error(&app.data, "leonida", &error.to_string()).await,
        }
        tokio::time::sleep(Duration::from_secs(REFRESH_SECONDS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readme_status_table_becomes_short_rows() {
        let readme = "intro\n## Project status\n\n**Early. The game does not boot yet.** More text.\n\n| What | Status |\n|---|---|\n| Booting, story mode, anything playable | ❌ No |\n| Shaders: RDNA2 → SPIR-V translation | ✅ **26,503 / 26,503 translated**: 2,236 VS, more |\n| Xbox D3D12 → Vulkan | ⚠️ In progress. A Vulkan-backed device |\n| [Models](docs/models.md) (OBJ with real names) | ✅ Exported. A scan |\n| SPIR-V validation (`spirv-val`) | ✅ **26,503 / 26,503 passed** in the LLVM 23.1.5 Linux rerun |\n| Shader translator checked on a real GPU | ⚠️ The hand-written GPU tests (vertex fetch, texture sampling) have been run on more than 30 GPU and driver combinations, with results in #166: NVIDIA |\n\n## Platforms\n| x | y |\n";
        let status = parse_status(readme);
        assert_eq!(status["headline"], "Early. The game does not boot yet.");
        let rows = status["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0]["mark"], "no");
        assert_eq!(status["boot"]["mark"], "no");
        assert_eq!(rows[1]["mark"], "done");
        assert_eq!(rows[1]["note"], "26,503 / 26,503 translated");
        assert_eq!(rows[2]["mark"], "partial");
        assert_eq!(rows[2]["note"], "In progress");
        assert_eq!(rows[3]["what"], "Models");
        assert_eq!(rows[4]["what"], "SPIR-V validation");
        assert_eq!(rows[4]["note"], "26,503 / 26,503 passed");
        assert_eq!(
            rows[5]["note"],
            "The hand-written GPU tests have been run on more than 30 GPU and driver combinations"
        );
    }

    #[test]
    fn the_boot_override_waits_for_a_readme_that_says_no() {
        let readme = parse_status(
            "## Project status\n\n**Early. The game does not boot yet.**\n\n| What | Status |\n|---|---|\n| Booting, story mode, anything playable | ❌ No |\n| Models | ✅ Exported |\n",
        );
        let overlay = json!({"mark":"partial","headline":"It boots!","note":"The .exe runs"});
        let status = apply_boot_override(&readme, &overlay);
        assert_eq!(status["boot"]["mark"], "partial");
        assert_eq!(status["headline"], "It boots!");
        assert_eq!(status["rows"][0]["mark"], "partial");
        assert_eq!(status["rows"][0]["note"], "The .exe runs");
        assert_eq!(status["rows"][1]["mark"], "done");
        // The README moved on: it wins.
        let mut moved = readme.clone();
        moved["boot"]["mark"] = json!("done");
        assert_eq!(
            apply_boot_override(&moved, &overlay)["boot"]["mark"],
            "done"
        );
        assert_eq!(apply_boot_override(&readme, &Value::Null), readme);
    }

    #[test]
    fn merge_commits_name_their_pull_request() {
        let number = |message: &str| MERGED_PR_RE.captures(message).map(|c| c[1].to_owned());
        assert_eq!(
            number("Merge remote-tracking branch 'origin/pr/9523'").as_deref(),
            Some("9523")
        );
        assert_eq!(
            number("Merge pull request #6693 from cynbun/awc").as_deref(),
            Some("6693")
        );
        assert_eq!(number("docs: fix a typo"), None);
    }

    #[test]
    fn star_gains_come_from_the_kept_counts() {
        let history = vec![
            json!([0, 100]),
            json!([3_000_000, 150]),
            json!([3_600_000, 160]),
        ];
        assert_eq!(gain_since(&history, 3_600_000, 3_600_000, 200), Some(100));
        assert_eq!(gain_since(&history, 3_600_000, 86_400_000, 200), None);
    }
}
