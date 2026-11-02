use std::env;

use anyhow::{Context, Result, anyhow};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let client_id = args.next().context(
        "usage: cargo run --manifest-path tools/Cargo.toml -- <client id> <client secret>",
    )?;
    let client_secret = args.next().context(
        "usage: cargo run --manifest-path tools/Cargo.toml -- <client id> <client secret>",
    )?;
    let redirect = "http://127.0.0.1:8765/";
    let mut consent = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    consent
        .query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair(
            "scope",
            "https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.force-ssl",
        )
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    println!("Open this page, signed in as the channel owner:\n\n{consent}\n");

    let listener = TcpListener::bind("127.0.0.1:8765").await?;
    let (mut socket, _) = listener.accept().await?;
    let mut request = vec![0u8; 8192];
    let n = socket.read(&mut request).await?;
    let request = std::str::from_utf8(&request[..n])?;
    let target = request
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .ok_or_else(|| anyhow!("invalid OAuth callback request"))?;
    let callback = Url::parse(&format!("http://127.0.0.1{target}"))?;
    let code = callback
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string());
    let Some(code) = code else {
        let body = "No code in this request.";
        let reply = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await?;
        return Err(anyhow!("Google did not return an authorization code"));
    };
    let client = reqwest::Client::new();
    let result: serde_json::Value = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if let Some(token) = result["refresh_token"].as_str() {
        let body = "Done, back to the terminal.";
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await?;
        println!("YOUTUBE_REFRESH_TOKEN={token}");
    } else {
        let body = "Authorization failed; check the terminal.";
        let reply = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await?;
        return Err(anyhow!(
            "Google token response did not include a refresh token: {result}"
        ));
    }
    Ok(())
}
