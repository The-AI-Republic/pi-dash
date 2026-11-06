//! "Is this <label>, last seen <date>? Replace it" after `pidash auth login`
//! (design `built_engine_as_runner` §4.10, acceptance scenario 15).
//!
//! When the daemon's data was wiped, the next sign-in mints a new machine
//! id and so creates a new machine. The server never guesses that it
//! replaces an old one (two computers can share a hostname); the user is
//! asked instead. The server lists the candidates — this user's offline
//! machines with the same `host_label` and no live session — and confirming
//! merges the old machine into the new one, moving its runner rows, pins
//! and pod membership. The daemon's reconcile then re-creates the moved
//! engine runners locally; own-agent runners cannot be rebuilt (their agent
//! and folder were local) and stay offline for the user to set up again.
//!
//! Best-effort throughout: a failure here is a note, never a failed login.

use std::io::{BufRead, Write};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Candidate {
    pub id: uuid::Uuid,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub host_label: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub last_seen_at: Option<String>,
    #[serde(default)]
    pub live_runner_count: Option<u64>,
}

impl Candidate {
    fn name(&self) -> &str {
        [&self.display_name, &self.label, &self.host_label]
            .into_iter()
            .find(|s| !s.trim().is_empty())
            .map(|s| s.trim())
            .unwrap_or("this computer")
    }

    /// `2026-11-05` from an RFC 3339 timestamp; "never" without one.
    fn last_seen(&self) -> String {
        match self.last_seen_at.as_deref() {
            Some(ts) => chrono::DateTime::parse_from_rfc3339(ts)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|_| ts.to_string()),
            None => "never".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CandidatesResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct MergeResult {
    #[serde(default)]
    pub moved_runner_ids: Vec<uuid::Uuid>,
}

/// What [`offer_replace`] did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The server listed no machine this one may replace.
    NoCandidates,
    /// The user answered no to every candidate.
    Declined,
    /// The old machine was merged into this one.
    Replaced { old: uuid::Uuid, moved: usize },
}

async fn fetch_candidates(
    client: &reqwest::Client,
    cloud_url: &str,
    machine_token: &str,
    machine_id: &uuid::Uuid,
) -> Result<Vec<Candidate>> {
    let url = format!("{cloud_url}/api/v1/runner/dev-machines/{machine_id}/replace-candidates/");
    let resp = client
        .get(&url)
        .bearer_auth(machine_token)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    Ok(resp
        .json::<CandidatesResponse>()
        .await
        .context("parsing the replace candidates")?
        .candidates)
}

async fn merge_into(
    client: &reqwest::Client,
    cloud_url: &str,
    machine_token: &str,
    old: &uuid::Uuid,
    new: &uuid::Uuid,
) -> Result<MergeResult> {
    let url = format!("{cloud_url}/api/v1/runner/dev-machines/{old}/merge-into/");
    let resp = client
        .post(&url)
        .bearer_auth(machine_token)
        .json(&serde_json::json!({ "target_dev_machine_id": new }))
        .send()
        .await
        .with_context(|| format!("POST {url}"))?;
    if resp.status().is_success() {
        return resp
            .json::<MergeResult>()
            .await
            .context("parsing the merge result");
    }
    let status = resp.status();
    let code = resp
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_default();
    anyhow::bail!("{}", merge_error_text(&code, status))
}

/// Plain text for the merge endpoint's error codes.
fn merge_error_text(code: &str, status: reqwest::StatusCode) -> String {
    match code {
        "source_machine_has_live_session" => {
            "That computer is still connected to Pi Dash, so it was not replaced. \
             If it is really gone, try again once it has been offline for a few minutes."
                .to_string()
        }
        "already_merged_into_another_machine" => {
            "That computer was already replaced by another one.".to_string()
        }
        "source_machine_revoked" => "That computer was already removed.".to_string(),
        "target_machine_not_live" => {
            "This computer's sign-in is no longer valid; run `pidash auth login` again.".to_string()
        }
        "dev_machine_not_found" => "That computer was not found on your account.".to_string(),
        "dev_machine_mismatch" | "cannot_merge_into_self" => {
            "Pi Dash refused to replace that computer from here.".to_string()
        }
        "" => format!("the server answered HTTP {status}"),
        other => format!("the server answered HTTP {status} ({other})"),
    }
}

/// Ask about each candidate in turn and merge the first one the user
/// confirms. The default answer is no: replacing moves the old machine's
/// runners and cannot be undone from the CLI.
pub(crate) async fn offer_replace<R: BufRead, W: Write>(
    client: &reqwest::Client,
    cloud_url: &str,
    machine_token: &str,
    machine_id: &uuid::Uuid,
    input: &mut R,
    output: &mut W,
) -> Result<Outcome> {
    let candidates = fetch_candidates(client, cloud_url, machine_token, machine_id).await?;
    if candidates.is_empty() {
        return Ok(Outcome::NoCandidates);
    }
    let _ = writeln!(output);
    let _ = writeln!(
        output,
        "This computer signed in as a new machine. Your account has {} with the same name.",
        if candidates.len() == 1 {
            "an offline machine"
        } else {
            "offline machines"
        }
    );
    for c in &candidates {
        let runners = match c.live_runner_count {
            Some(1) => " (1 runner)".to_string(),
            Some(n) if n > 1 => format!(" ({n} runners)"),
            _ => String::new(),
        };
        let prompt = format!(
            "Is this {}, last seen {}{runners}? Replace it",
            c.name(),
            c.last_seen()
        );
        if !crate::util::confirm::ask_with_streams(&prompt, false, input, output) {
            continue;
        }
        let result = merge_into(client, cloud_url, machine_token, &c.id, machine_id).await?;
        return Ok(Outcome::Replaced {
            old: c.id,
            moved: result.moved_runner_ids.len(),
        });
    }
    Ok(Outcome::Declined)
}

/// The login's step: ask on a terminal, print what happened, never fail.
pub(crate) async fn offer_replace_after_login(
    client: &reqwest::Client,
    cloud_url: &str,
    machine_token: &str,
    machine_id: &uuid::Uuid,
) {
    let stdin = std::io::stdin();
    let outcome = offer_replace(
        client,
        cloud_url,
        machine_token,
        machine_id,
        &mut stdin.lock(),
        &mut std::io::stdout(),
    )
    .await;
    match outcome {
        Ok(Outcome::NoCandidates) => {}
        Ok(Outcome::Declined) => {
            println!(
                "  The old machine stays offline. You can remove it, or replace it later, from AI Dev Machines on the web."
            );
        }
        Ok(Outcome::Replaced { moved, .. }) => {
            let runners = if moved == 1 { "runner" } else { "runners" };
            println!("✓ Replaced the old machine: {moved} {runners} moved to this computer.");
            println!(
                "  Built-in engine runners come back on their own. Runners that used your own agent need setting up again (`pidash runner add`)."
            );
        }
        Err(e) => {
            tracing::warn!("replace prompt failed: {e:#}");
            println!("  (Could not check for an old machine to replace: {e:#})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

    /// One response per request (`Connection: close`), chosen by
    /// `method path`. Records `(request line, authorization, body)`.
    async fn fake_cloud(routes: Vec<(&'static str, u16, String)>) -> (String, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let seen_srv = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break None;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break Some(i);
                    }
                };
                let Some(head_end) = head_end else { continue };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let header = |name: &str| {
                    head.lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                        })
                        .unwrap_or_default()
                };
                let len: usize = header("content-length").parse().unwrap_or(0);
                while buf.len() < head_end + 4 + len {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let line = head.lines().next().unwrap_or_default().to_string();
                let body = String::from_utf8_lossy(&buf[head_end + 4..]).to_string();
                seen_srv
                    .lock()
                    .unwrap()
                    .push((line.clone(), header("authorization"), body));
                let (status, resp) = routes
                    .iter()
                    .find(|(route, _, _)| line.starts_with(route))
                    .map(|(_, s, b)| (*s, b.clone()))
                    .unwrap_or((404, "{}".to_string()));
                let out = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}",
                    resp.len()
                );
                let _ = socket.write_all(out.as_bytes()).await;
            }
        });
        (url, seen)
    }

    fn candidates_body(ids: &[uuid::Uuid]) -> String {
        let list: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                serde_json::json!({
                    "id": id, "label": format!("old-{i}"), "host_label": "rich-mbp",
                    "display_name": "", "last_seen_at": "2026-10-01T10:00:00+00:00",
                    "live_runner_count": 2
                })
            })
            .collect();
        serde_json::json!({ "candidates": list }).to_string()
    }

    async fn run(url: &str, new: uuid::Uuid, answers: &str) -> (Result<Outcome>, String) {
        let client = reqwest::Client::new();
        let mut input = std::io::Cursor::new(answers.as_bytes().to_vec());
        let mut out = Vec::new();
        let r = offer_replace(&client, url, "mt_new", &new, &mut input, &mut out).await;
        (r, String::from_utf8(out).unwrap())
    }

    #[tokio::test]
    async fn confirming_merges_the_old_machine_into_this_one_with_its_token() {
        let new = uuid::Uuid::new_v4();
        let old = uuid::Uuid::new_v4();
        let moved = uuid::Uuid::new_v4();
        let (url, seen) = fake_cloud(vec![
            (
                "GET /api/v1/runner/dev-machines/",
                200,
                candidates_body(&[old]),
            ),
            (
                "POST /api/v1/runner/dev-machines/",
                200,
                serde_json::json!({ "moved_runner_ids": [moved], "collisions": [] }).to_string(),
            ),
        ])
        .await;
        let (r, out) = run(&url, new, "y\n").await;
        assert_eq!(r.unwrap(), Outcome::Replaced { old, moved: 1 });
        assert!(out.contains("Is this old-0, last seen "), "{out}");
        assert!(out.contains("(2 runners)? Replace it [y/N]"), "{out}");
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen[0].0,
            format!("GET /api/v1/runner/dev-machines/{new}/replace-candidates/ HTTP/1.1")
        );
        assert_eq!(seen[0].1, "Bearer mt_new");
        assert_eq!(
            seen[1].0,
            format!("POST /api/v1/runner/dev-machines/{old}/merge-into/ HTTP/1.1")
        );
        assert_eq!(seen[1].1, "Bearer mt_new");
        let body: serde_json::Value = serde_json::from_str(&seen[1].2).unwrap();
        assert_eq!(body["target_dev_machine_id"], new.to_string());
    }

    #[tokio::test]
    async fn the_default_answer_is_no_and_nothing_is_merged() {
        let new = uuid::Uuid::new_v4();
        let (url, seen) = fake_cloud(vec![(
            "GET /api/v1/runner/dev-machines/",
            200,
            candidates_body(&[uuid::Uuid::new_v4(), uuid::Uuid::new_v4()]),
        )])
        .await;
        // Enter on the first, closed stdin on the second.
        let (r, out) = run(&url, new, "\n").await;
        assert_eq!(r.unwrap(), Outcome::Declined);
        assert_eq!(out.matches("Replace it [y/N]").count(), 2, "{out}");
        assert_eq!(seen.lock().unwrap().len(), 1, "no merge call");
    }

    #[tokio::test]
    async fn second_candidate_can_be_chosen() {
        let new = uuid::Uuid::new_v4();
        let ids = [uuid::Uuid::new_v4(), uuid::Uuid::new_v4()];
        let (url, seen) = fake_cloud(vec![
            (
                "GET /api/v1/runner/dev-machines/",
                200,
                candidates_body(&ids),
            ),
            (
                "POST /api/v1/runner/dev-machines/",
                200,
                "{\"moved_runner_ids\": []}".into(),
            ),
        ])
        .await;
        let (r, _) = run(&url, new, "n\ny\n").await;
        assert_eq!(
            r.unwrap(),
            Outcome::Replaced {
                old: ids[1],
                moved: 0
            }
        );
        assert!(seen.lock().unwrap()[1].0.contains(&ids[1].to_string()));
    }

    #[tokio::test]
    async fn no_candidates_asks_nothing() {
        let (url, _) = fake_cloud(vec![(
            "GET /api/v1/runner/dev-machines/",
            200,
            "{\"candidates\": []}".into(),
        )])
        .await;
        let (r, out) = run(&url, uuid::Uuid::new_v4(), "y\n").await;
        assert_eq!(r.unwrap(), Outcome::NoCandidates);
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn merge_refusal_reads_as_plain_text() {
        let (url, _) = fake_cloud(vec![
            (
                "GET /api/v1/runner/dev-machines/",
                200,
                candidates_body(&[uuid::Uuid::new_v4()]),
            ),
            (
                "POST /api/v1/runner/dev-machines/",
                409,
                "{\"error\": \"source_machine_has_live_session\"}".into(),
            ),
        ])
        .await;
        let (r, _) = run(&url, uuid::Uuid::new_v4(), "yes\n").await;
        let err = r.unwrap_err().to_string();
        assert!(
            err.starts_with("That computer is still connected to Pi Dash"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn an_older_server_without_the_endpoint_is_an_error_not_a_prompt() {
        let (url, _) = fake_cloud(vec![]).await;
        let (r, out) = run(&url, uuid::Uuid::new_v4(), "y\n").await;
        assert!(r.is_err());
        assert!(out.is_empty());
    }
}
