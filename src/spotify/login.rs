//! In-process Spotify OAuth login (PKCE authorization-code flow), shared by
//! the TUI re-login recovery screen and `looper spotify login`.
//!
//! Driven directly with the `oauth2` crate instead of `librespot-oauth` so
//! the auth URL is programmatically available (the TUI shows it), nothing is
//! printed to stdout (the TUI owns it), and the redirect listener is owned
//! here — non-blocking with a cancel flag, so Esc frees port 8898 and an
//! immediate retry can re-bind.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};
use librespot_core::config::SessionConfig;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope,
    TokenResponse, TokenUrl,
};

use super::OAUTH_REDIRECT_URI;

/// Progress of a login attempt, streamed to the caller. `Done(Ok)` carries the
/// username; `Done(Err)` a display message.
pub enum LoginPhase {
    WaitingForBrowser { auth_url: String },
    Connecting,
    Done(Result<String, String>),
}

/// A running login attempt. Dropping the handle abandons it; [`cancel`]
/// additionally makes the listener thread exit promptly and free port 8898.
///
/// [`cancel`]: LoginHandle::cancel
pub struct LoginHandle {
    pub phases: Receiver<LoginPhase>,
    cancel: Arc<AtomicBool>,
}

impl LoginHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Start the OAuth login on a background thread.
pub fn start_login() -> LoginHandle {
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        let result = run_login(&tx, &flag);
        let _ = tx.send(LoginPhase::Done(result.map_err(|e| e.to_string())));
    });
    LoginHandle { phases: rx, cancel }
}

fn run_login(tx: &Sender<LoginPhase>, cancel: &AtomicBool) -> Result<String> {
    // Bind before opening the browser so a port conflict fails fast.
    let listener = TcpListener::bind("127.0.0.1:8898")
        .map_err(|e| eyre!("couldn't listen for the Spotify redirect on port 8898: {e}"))?;
    listener.set_nonblocking(true)?;

    let client = BasicClient::new(ClientId::new(SessionConfig::default().client_id))
        .set_auth_uri(AuthUrl::new(
            "https://accounts.spotify.com/authorize".to_string(),
        )?)
        .set_token_uri(TokenUrl::new(
            "https://accounts.spotify.com/api/token".to_string(),
        )?)
        .set_redirect_uri(RedirectUrl::new(OAUTH_REDIRECT_URI.to_string())?);

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("streaming".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    let _ = tx.send(LoginPhase::WaitingForBrowser {
        auth_url: auth_url.to_string(),
    });
    open::that_in_background(auth_url.as_str());

    // Non-blocking accept loop so Esc (the cancel flag) can free the port.
    let mut stream = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(eyre!("login cancelled"));
        }
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(eyre!("Spotify redirect listener failed: {e}")),
        }
    };
    // Accepted sockets inherit non-blocking on macOS/BSD; undo it for the
    // short synchronous read/write below.
    stream.set_nonblocking(false)?;

    let mut request_line = String::new();
    BufReader::new(&stream)
        .read_line(&mut request_line)
        .map_err(|e| eyre!("failed to read the Spotify redirect: {e}"))?;
    let (code, state) = code_and_state_from_request_line(&request_line)
        .ok_or_else(|| eyre!("the Spotify redirect didn't include an auth code"))?;
    // Reject a forged redirect (e.g. a malicious local page hitting the
    // listener with an attacker's code): the state must round-trip.
    if state.as_deref() != Some(csrf.secret().as_str()) {
        return Err(eyre!("the Spotify redirect failed the state check"));
    }

    let body = "Logged in - you can close this tab and go back to looper.";
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());

    let _ = tx.send(LoginPhase::Connecting);
    let http = reqwest::blocking::Client::new();
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(pkce_verifier)
        .request(&http)
        .map_err(|e| eyre!("Spotify token exchange failed: {e}"))?;

    super::connect_with_token(token.access_token().secret().to_string())
}

/// Extract the `code` and `state` query parameters from the redirect request
/// line (`GET /login?code=...&state=... HTTP/1.1`). `None` without a code;
/// the caller verifies the state against the CSRF token it generated.
fn code_and_state_from_request_line(line: &str) -> Option<(String, Option<String>)> {
    let path = line.split_whitespace().nth(1)?;
    let url = url::Url::parse(&format!("http://localhost{path}")).ok()?;
    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }
    Some((code?, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_and_state_from_redirect_request() {
        assert_eq!(
            code_and_state_from_request_line("GET /login?code=AQBzk3example&state=xyz HTTP/1.1\r\n"),
            Some(("AQBzk3example".to_string(), Some("xyz".to_string())))
        );
        assert_eq!(
            code_and_state_from_request_line("GET /login?code=AQBzk3example HTTP/1.1\r\n"),
            Some(("AQBzk3example".to_string(), None))
        );
        assert_eq!(
            code_and_state_from_request_line("GET /login?error=access_denied HTTP/1.1\r\n"),
            None
        );
        assert_eq!(code_and_state_from_request_line(""), None);
    }
}
