# Spotify Re-Login Recovery Flow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When Spotify resolve fails because the cached login expired/revoked, show a dedicated modal that runs an in-TUI browser OAuth re-login and then resumes playing the originally requested item.

**Architecture:** Classify auth failures with a typed `SpotifyAuthError` at the librespot boundary; carry the classification through the resolver thread as a `ResolveFailure` enum instead of a flat `String`; add a login prompt modal + waiting screen in the TUI; drive the PKCE OAuth flow directly with the `oauth2` crate on a background thread reporting phases over an `mpsc` channel (cancellable, auth URL displayable, no stdout writes). One login code path shared by the TUI and `looper spotify login`.

**Tech Stack:** Rust, ratatui/crossterm, librespot 0.8, oauth2 5.0, reqwest (blocking, already a direct dep), open 5, url 2.

**Spec:** `docs/superpowers/specs/2026-08-11-spotify-relogin-flow-design.md`

## Global Constraints

- Do NOT run `cargo update`. `vergen` is pinned to 9.0.6 in `Cargo.lock`; adding direct deps already present transitively (`oauth2` 5.0.0, `open` 5.3.5, `url` 2.5.8) must not change lockfile versions.
- OAuth redirect stays `http://127.0.0.1:8898/login` (existing `OAUTH_REDIRECT_URI` in `src/spotify/mod.rs`); scope stays `streaming`; client id comes from `SessionConfig::default().client_id`.
- The login thread must never `println!` (it runs under the TUI).
- Key-handling conventions match existing modals: any-key = back, `q`/Ctrl-C = quit.
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`; stage files by name.

---

### Task 1: Typed auth-failure classification in `src/spotify/mod.rs`

**Files:**
- Modify: `src/spotify/mod.rs` (connect_session at ~line 416; add types + helpers near it)
- Test: inline `#[cfg(test)] mod tests` already at bottom of `src/spotify/mod.rs`

**Interfaces:**
- Produces: `pub struct SpotifyAuthError(pub String)` (implements `std::error::Error`); `pub fn auth_error(err: &color_eyre::eyre::Report) -> Option<String>` returning the auth message when the chain contains `SpotifyAuthError`; internal `fn classify_connect_error(e: librespot_core::Error) -> color_eyre::eyre::Report`.

- [ ] **Step 1: Write the failing tests** (append inside the existing `mod tests` in `src/spotify/mod.rs`)

```rust
    #[test]
    fn classifies_auth_kinds_as_auth_errors() {
        let denied = classify_connect_error(librespot_core::Error::permission_denied("bad creds"));
        assert!(auth_error(&denied).is_some());

        let unauth = classify_connect_error(librespot_core::Error::unauthenticated("token dead"));
        assert!(auth_error(&unauth).is_some());
    }

    #[test]
    fn network_failures_are_not_auth_errors() {
        let network = classify_connect_error(librespot_core::Error::unavailable("no route"));
        assert!(auth_error(&network).is_none());
        assert!(network.to_string().contains("failed to connect to Spotify"));
    }

    #[test]
    fn auth_error_carries_its_message() {
        let report = color_eyre::eyre::Report::new(SpotifyAuthError("not logged in".into()));
        assert_eq!(auth_error(&report).as_deref(), Some("not logged in"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib spotify -- classifies_auth network_failures auth_error_carries 2>&1 | tail -20` — actually filters are one-per-invocation; run `cargo test spotify::tests 2>&1 | tail -20`
Expected: compile FAIL — `SpotifyAuthError`, `classify_connect_error`, `auth_error` not found.

- [ ] **Step 3: Implement**

In `src/spotify/mod.rs`, above `connect_session`:

```rust
/// The cached Spotify login is missing, expired, or revoked. Typed so the TUI
/// can offer re-login instead of the generic "track unavailable" modal.
#[derive(Debug)]
pub struct SpotifyAuthError(pub String);

impl std::fmt::Display for SpotifyAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SpotifyAuthError {}

/// The auth-failure message if `err`'s chain contains a [`SpotifyAuthError`].
pub fn auth_error(err: &color_eyre::eyre::Report) -> Option<String> {
    err.chain()
        .find_map(|e| e.downcast_ref::<SpotifyAuthError>())
        .map(|e| e.0.clone())
}

/// librespot maps LoginFailed → PermissionDenied and other auth failures →
/// Unauthenticated; those mean the saved login is dead. Everything else
/// (network down, AP unreachable) keeps its generic wording.
fn classify_connect_error(e: librespot_core::Error) -> color_eyre::eyre::Report {
    use librespot_core::error::ErrorKind;
    match e.kind {
        ErrorKind::PermissionDenied | ErrorKind::Unauthenticated => {
            color_eyre::eyre::Report::new(SpotifyAuthError(format!(
                "Spotify rejected the saved login: {e}"
            )))
        }
        _ => eyre!("failed to connect to Spotify: {e}"),
    }
}
```

Rewrite `connect_session` to use both:

```rust
/// Connect a fresh session from cached OAuth credentials. Auth failures
/// (missing/expired/revoked credentials) surface as [`SpotifyAuthError`] so the
/// TUI can offer re-login. `Session::new` calls `Handle::current()`, so it must
/// be built inside the runtime.
fn connect_session(runtime: &Runtime) -> Result<Session> {
    let cache = open_cache()?;
    let credentials = cache.credentials().ok_or_else(|| {
        color_eyre::eyre::Report::new(SpotifyAuthError("not logged in to Spotify".to_string()))
    })?;
    runtime
        .block_on(async move {
            let session = Session::new(SessionConfig::default(), Some(cache));
            session.connect(credentials, true).await.map(|()| session)
        })
        .map_err(classify_connect_error)
}
```

(The old `.wrap_err("failed to connect to Spotify (is your Premium login still valid?)")` goes away; drop the now-unused `WrapErr` import only if nothing else in the file uses it — `open_cache` and `art_dir` do use it, so keep.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test spotify::tests 2>&1 | tail -20`
Expected: PASS (all, including the pre-existing URL-parsing tests).

- [ ] **Step 5: Commit**

```bash
git add src/spotify/mod.rs
git commit -m "feat(spotify): classify expired/revoked login as typed auth error"
```

---

### Task 2: Shared OAuth login flow (`src/spotify/login.rs`) + CLI rewrite

**Files:**
- Modify: `Cargo.toml` (add `oauth2`, `open`, `url` direct deps; remove `librespot-oauth`)
- Create: `src/spotify/login.rs`
- Modify: `src/spotify/mod.rs` (declare module, re-export, add `connect_with_token`, rewrite `login()`, remove `librespot_oauth` import and old `login()` body)
- Test: inline `#[cfg(test)]` in `src/spotify/login.rs`

**Interfaces:**
- Consumes: `open_cache()`, `ctx()`, `OAUTH_REDIRECT_URI`, `SessionConfig`, `Credentials::with_access_token` (all existing in `src/spotify/mod.rs`).
- Produces:
  - `pub enum LoginPhase { WaitingForBrowser { auth_url: String }, Connecting, Done(Result<String, String>) }` (`Ok` = username, `Err` = display message)
  - `pub struct LoginHandle { pub phases: std::sync::mpsc::Receiver<LoginPhase>, .. }` with `pub fn cancel(&self)`
  - `pub fn start_login() -> LoginHandle`
  - re-exported from `crate::spotify` as `spotify::{start_login, LoginHandle, LoginPhase}`
  - `pub(crate) fn connect_with_token(access_token: String) -> Result<String>` in `mod.rs` (connects, installs session into the shared slot, returns username)

- [ ] **Step 1: Dependency edits in `Cargo.toml`**

Replace the `librespot-oauth = "0.8"` line with (it stays in the tree transitively via librespot-core):

```toml
# OAuth for `looper spotify login` and the in-TUI re-login flow. Driven
# directly (not via librespot-oauth) so the TUI can display the auth URL,
# cancel the redirect listener cleanly, and avoid stray stdout writes. All
# three are already in the tree transitively — same compiled versions.
oauth2 = { version = "5.0", features = ["reqwest", "reqwest-blocking"] }
open = "5"
url = "2"
```

Run: `cargo build 2>&1 | tail -5` — expect success, and `git diff Cargo.lock` should only add/remove direct-dep edges, not change any versions (vergen stays 9.0.6).

- [ ] **Step 2: Write the failing test**

Create `src/spotify/login.rs` with just the parser + test first:

```rust
//! In-process Spotify OAuth login (PKCE authorization-code flow), shared by
//! the TUI re-login recovery screen and `looper spotify login`.
//!
//! Driven directly with the `oauth2` crate instead of `librespot-oauth` so
//! the auth URL is programmatically available (the TUI shows it), nothing is
//! printed to stdout (the TUI owns it), and the redirect listener is owned
//! here — non-blocking with a cancel flag, so Esc frees port 8898 and an
//! immediate retry can re-bind.

/// Extract the `code` query parameter from the redirect request line
/// (`GET /login?code=... HTTP/1.1`).
fn code_from_request_line(line: &str) -> Option<String> {
    let path = line.split_whitespace().nth(1)?;
    let url = url::Url::parse(&format!("http://localhost{path}")).ok()?;
    url.query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_code_from_redirect_request() {
        assert_eq!(
            code_from_request_line("GET /login?code=AQBzk3example&state=xyz HTTP/1.1\r\n"),
            Some("AQBzk3example".to_string())
        );
        assert_eq!(
            code_from_request_line("GET /login?error=access_denied HTTP/1.1\r\n"),
            None
        );
        assert_eq!(code_from_request_line(""), None);
    }
}
```

Declare in `src/spotify/mod.rs` next to `mod search;`:

```rust
mod login;
```

- [ ] **Step 3: Run test to verify it fails, then passes**

Run: `cargo test spotify::login 2>&1 | tail -10`
Expected: PASS (parser is implemented with the test; the failing state was the missing module — if it compiles and passes, continue).

- [ ] **Step 4: Implement the flow thread**

Complete `src/spotify/login.rs` (above the test mod):

```rust
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
    let (auth_url, _csrf) = client
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
    let code = code_from_request_line(&request_line)
        .ok_or_else(|| eyre!("the Spotify redirect didn't include an auth code"))?;

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
```

In `src/spotify/mod.rs`:

1. Replace `use librespot_oauth::OAuthClientBuilder;` with nothing (delete the line). Add re-export next to the search one:

```rust
pub use login::{start_login, LoginHandle, LoginPhase};
```

2. Add below `connect_session`:

```rust
/// Connect a brand-new session from a fresh OAuth access token, persist
/// reusable credentials into the cache (`store_credentials = true`), and
/// install the session into the shared slot so the next resolve reuses it.
/// Returns the username.
pub(crate) fn connect_with_token(access_token: String) -> Result<String> {
    let ctx = ctx()?;
    let cache = open_cache()?;
    let session = ctx
        .runtime
        .block_on(async move {
            let session = Session::new(SessionConfig::default(), Some(cache));
            session
                .connect(Credentials::with_access_token(access_token), true)
                .await
                .map(|()| session)
        })
        .map_err(|e| eyre!("Spotify login failed (Premium required): {e}"))?;
    let username = session.username();
    *ctx.session.lock().unwrap() = Some(session);
    Ok(username)
}
```

3. Replace the whole body of `pub fn login()` (the CLI path) with the shared flow:

```rust
/// Run the OAuth browser flow and cache reusable credentials (CLI path;
/// the TUI drives [`start_login`] itself).
pub fn login() -> Result<()> {
    let handle = start_login();
    loop {
        match handle.phases.recv() {
            Ok(LoginPhase::WaitingForBrowser { auth_url }) => {
                println!("Opening your browser to authorize looper with Spotify...");
                println!("If it doesn't open, browse to:\n{auth_url}");
            }
            Ok(LoginPhase::Connecting) => println!("Connecting to Spotify..."),
            Ok(LoginPhase::Done(Ok(username))) => {
                println!(
                    "Logged in to Spotify as {username}. Credentials cached — you won't need to do this again."
                );
                return Ok(());
            }
            Ok(LoginPhase::Done(Err(message))) => return Err(eyre!(message)),
            Err(_) => return Err(eyre!("Spotify login stopped unexpectedly")),
        }
    }
}
```

Remove imports that only the old `login()` used (`OAuthClientBuilder`; keep `Credentials`, `SessionConfig` — both still used).

- [ ] **Step 5: Build + run tests**

Run: `cargo build 2>&1 | tail -5 && cargo test spotify 2>&1 | tail -10`
Expected: builds clean (no unused-import warnings), all spotify tests PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/spotify/login.rs src/spotify/mod.rs
git commit -m "feat(spotify): own the OAuth login flow, cancellable and TUI-drivable"
```

---

### Task 3: Carry auth classification through the resolver thread

**Files:**
- Modify: `src/play_loop.rs` (`resolve_url_with_startup` ~line 488, `ResolveStartupOutcome` ~line 443, `play_file_session` ~line 369, `handle_unresolvable_replay` ~line 454)

**Interfaces:**
- Consumes: `crate::spotify::auth_error(&Report) -> Option<String>` (Task 1).
- Produces: `ResolveStartupOutcome::SpotifyLoginRequired(String)`; `handle_unresolvable_replay(..., message: &str)`; a `ResolveFailure` enum private to `play_loop.rs`. Task 5 consumes the `SpotifyLoginRequired` arm (it temporarily returns to history like `Failed` until then).

- [ ] **Step 1: Implement**

Add near `ResolveStartupOutcome`:

```rust
/// Why the resolver thread failed, classified before the error is flattened
/// to a string so the auth case can offer re-login.
enum ResolveFailure {
    /// The Spotify login is missing/expired/revoked; payload is the detail.
    AuthRequired(String),
    Other(String),
}
```

Extend the outcome enum:

```rust
enum ResolveStartupOutcome {
    Resolved(Option<Vec<TrackInfo>>),
    Quit,
    /// The remote resolver reported the URL is unplayable (private, removed,
    /// region-locked, expired live stream). Recoverable: surface it in the TUI
    /// instead of crashing the whole app.
    Failed(String),
    /// Resolve failed because the Spotify login is dead; offer re-login.
    SpotifyLoginRequired(String),
}
```

In `resolve_url_with_startup`, change the resolver thread and the receive arm:

```rust
    thread::spawn(move || {
        let result = plugin::resolve_url(&url).map_err(|err| {
            match crate::spotify::auth_error(&err) {
                Some(detail) => ResolveFailure::AuthRequired(detail),
                None => ResolveFailure::Other(err.to_string()),
            }
        });
        let _ = sender.send(result);
    });
```

```rust
            Ok(Err(ResolveFailure::AuthRequired(detail))) => {
                return Ok(ResolveStartupOutcome::SpotifyLoginRequired(detail))
            }
            Ok(Err(ResolveFailure::Other(message))) => {
                return Ok(ResolveStartupOutcome::Failed(message))
            }
```

In `play_file_session`, use the real message and add a placeholder arm (Task 5 replaces it):

```rust
                ResolveStartupOutcome::Failed(message) => {
                    handle_unresolvable_replay(
                        terminal,
                        title_state,
                        &storage,
                        &current_url,
                        &message,
                    )?;
                    push_replica_best_effort(&storage);
                    return Ok(SessionOutcome::BackToHistory);
                }
                ResolveStartupOutcome::SpotifyLoginRequired(message) => {
                    // Until Task 5 wires the login flow, fall back to the
                    // generic modal so the build stays shippable.
                    handle_unresolvable_replay(
                        terminal,
                        title_state,
                        &storage,
                        &current_url,
                        &message,
                    )?;
                    push_replica_best_effort(&storage);
                    return Ok(SessionOutcome::BackToHistory);
                }
```

In `handle_unresolvable_replay`, add the parameter and prefer the real message:

```rust
fn handle_unresolvable_replay(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    storage: &SharedStorage,
    replay_target: &str,
    message: &str,
) -> Result<()> {
```

and replace the hardcoded detail line:

```rust
    let detail = if message.is_empty() {
        "This track may be private, removed, or region-locked.".to_string()
    } else {
        truncate_title(message, 62)
    };
```

(pass `&detail` where `detail` was used in the draw call).

- [ ] **Step 2: Build + test**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -10`
Expected: clean build, all tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/play_loop.rs
git commit -m "feat(tui): classify Spotify auth failures at resolve and show real error detail"
```

---

### Task 4: Login prompt modal + waiting screen in `src/tui.rs`

**Files:**
- Modify: `src/tui.rs` (add `Wrap` to the `ratatui::widgets` import at line 13; add state struct + two draw fns next to `draw_replay_error` ~line 653)

**Interfaces:**
- Produces: `pub struct SpotifyLoginScreenState { pub phase_label: String, pub auth_url: Option<String>, pub frame_count: u64 }`; `pub fn draw_spotify_login_prompt(frame: &mut ratatui::Frame, detail: &str)`; `pub fn draw_spotify_login_wait(frame: &mut ratatui::Frame, state: &SpotifyLoginScreenState)`. Task 5 consumes all three.

- [ ] **Step 1: Implement**

Change the widgets import:

```rust
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
```

Add below `draw_replay_error`:

```rust
/// Modal shown when resolve failed because the Spotify login is dead.
pub fn draw_spotify_login_prompt(frame: &mut ratatui::Frame, detail: &str) {
    frame.render_widget(Clear, frame.area());
    let area = centered_area(frame.area(), 66, 9);
    let lines = vec![
        Line::from(vec![Span::styled(
            "♪  Spotify login needed",
            Style::default()
                .fg(Color::Rgb(120, 220, 130))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Your Spotify login has expired or been revoked.",
            Style::default()
                .fg(Color::Rgb(230, 230, 240))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            detail.to_string(),
            Style::default().fg(Color::Rgb(170, 170, 190)),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "[enter]",
                Style::default()
                    .fg(Color::Rgb(255, 180, 80))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " log in again    ",
                Style::default().fg(Color::Rgb(150, 150, 170)),
            ),
            Span::styled(
                "[any key]",
                Style::default()
                    .fg(Color::Rgb(255, 180, 80))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" back    ", Style::default().fg(Color::Rgb(150, 150, 170))),
            Span::styled(
                "[q]",
                Style::default()
                    .fg(Color::Rgb(255, 180, 80))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" quit", Style::default().fg(Color::Rgb(150, 150, 170))),
        ]),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" spotify login ")
        .style(Style::default().fg(Color::Rgb(90, 150, 100)));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The in-progress login screen: spinner + phase, with the auth URL as a
/// fallback if the browser didn't open.
pub struct SpotifyLoginScreenState {
    pub phase_label: String,
    pub auth_url: Option<String>,
    pub frame_count: u64,
}

pub fn draw_spotify_login_wait(frame: &mut ratatui::Frame, state: &SpotifyLoginScreenState) {
    const SPINNER: [char; 4] = ['◐', '◓', '◑', '◒'];
    let spinner = SPINNER[(state.frame_count / 4) as usize % SPINNER.len()];
    frame.render_widget(Clear, frame.area());
    let area = centered_area(frame.area(), 76, 12);
    let mut lines = vec![
        Line::from(vec![Span::styled(
            format!("{spinner}  {}", state.phase_label),
            Style::default()
                .fg(Color::Rgb(120, 220, 130))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];
    if let Some(auth_url) = &state.auth_url {
        lines.push(Line::from(vec![Span::styled(
            "A browser window should have opened. If not, open:",
            Style::default().fg(Color::Rgb(170, 170, 190)),
        )]));
        lines.push(Line::from(vec![Span::styled(
            auth_url.to_string(),
            Style::default().fg(Color::Rgb(150, 170, 220)),
        )]));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![
        Span::styled(
            "[esc]",
            Style::default()
                .fg(Color::Rgb(255, 180, 80))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", Style::default().fg(Color::Rgb(150, 150, 170))),
    ]));
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" spotify login ")
        .style(Style::default().fg(Color::Rgb(90, 150, 100)));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        area,
    );
}
```

- [ ] **Step 2: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: clean build. (Draw fns have no unit tests, consistent with the rest of `tui.rs`; the wait screen is exercised manually in Task 6. Note: with `Wrap`, a long URL flows onto continuation lines inside the 76×12 box.)

- [ ] **Step 3: Commit**

```bash
git add src/tui.rs
git commit -m "feat(tui): Spotify login prompt modal and waiting screen"
```

---

### Task 5: Wire the login flow into `play_file_session`

**Files:**
- Modify: `src/play_loop.rs` (replace the Task-3 placeholder arm; add `handle_spotify_login` + `run_spotify_login_screen` next to `handle_unresolvable_replay`; extend the tui import at ~line 37 with `draw_spotify_login_prompt`, `draw_spotify_login_wait`, `SpotifyLoginScreenState`)

**Interfaces:**
- Consumes: `spotify::{start_login, LoginPhase}` (Task 2); `draw_spotify_login_prompt`, `draw_spotify_login_wait`, `SpotifyLoginScreenState` (Task 4).
- Produces: the finished user flow; `LoginOutcome`/`LoginScreenOutcome` enums private to `play_loop.rs`.

- [ ] **Step 1: Implement**

Replace the placeholder `SpotifyLoginRequired` arm in `play_file_session`:

```rust
                ResolveStartupOutcome::SpotifyLoginRequired(message) => {
                    match handle_spotify_login(terminal, title_state, &message)? {
                        // Login succeeded: re-resolve the same URL against the
                        // fresh session and play what originally failed.
                        LoginOutcome::Retry => continue,
                        LoginOutcome::Back => {
                            push_replica_best_effort(&storage);
                            return Ok(SessionOutcome::BackToHistory);
                        }
                        LoginOutcome::Quit => {
                            push_replica_best_effort(&storage);
                            return Ok(SessionOutcome::Quit);
                        }
                    }
                }
```

Add below `handle_unresolvable_replay`:

```rust
enum LoginOutcome {
    Retry,
    Back,
    Quit,
}

enum LoginScreenOutcome {
    LoggedIn,
    Cancelled,
    Failed(String),
    Quit,
}

/// The "Spotify login needed" modal: `enter` runs the browser OAuth flow,
/// any other key returns to the history browser, `q`/Ctrl-C quits. A failed
/// attempt loops back here with the failure as the detail line.
fn handle_spotify_login(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    detail: &str,
) -> Result<LoginOutcome> {
    let mut detail = truncate_title(detail, 62);
    title_state.set("looper — Spotify login needed".to_string())?;
    loop {
        terminal.draw(|frame| draw_spotify_login_prompt(frame, &detail))?;
        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Enter, _) => {
                        match run_spotify_login_screen(terminal, title_state)? {
                            LoginScreenOutcome::LoggedIn => return Ok(LoginOutcome::Retry),
                            LoginScreenOutcome::Cancelled => {
                                title_state.set("looper — Spotify login needed".to_string())?;
                            }
                            LoginScreenOutcome::Failed(message) => {
                                detail = truncate_title(&message, 62);
                                title_state.set("looper — Spotify login needed".to_string())?;
                            }
                            LoginScreenOutcome::Quit => return Ok(LoginOutcome::Quit),
                        }
                    }
                    (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        return Ok(LoginOutcome::Quit)
                    }
                    _ => return Ok(LoginOutcome::Back),
                }
            }
        }
    }
}

/// Drive one OAuth login attempt: background thread does the work, this loop
/// animates the waiting screen and polls its phase channel (the same pattern
/// as the resolver thread in `resolve_url_with_startup`). Esc cancels — the
/// flag makes the listener thread exit and free port 8898.
fn run_spotify_login_screen(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
) -> Result<LoginScreenOutcome> {
    let handle = spotify::start_login();
    let mut screen = SpotifyLoginScreenState {
        phase_label: "starting Spotify login...".to_string(),
        auth_url: None,
        frame_count: 0,
    };
    title_state.set("looper — Spotify login".to_string())?;
    loop {
        screen.frame_count += 1;
        terminal.draw(|frame| draw_spotify_login_wait(frame, &screen))?;

        match handle.phases.try_recv() {
            Ok(spotify::LoginPhase::WaitingForBrowser { auth_url }) => {
                screen.phase_label = "finish logging in to Spotify in your browser...".to_string();
                screen.auth_url = Some(auth_url);
            }
            Ok(spotify::LoginPhase::Connecting) => {
                screen.phase_label = "connecting to Spotify...".to_string();
                screen.auth_url = None;
            }
            Ok(spotify::LoginPhase::Done(Ok(_username))) => {
                return Ok(LoginScreenOutcome::LoggedIn)
            }
            Ok(spotify::LoginPhase::Done(Err(message))) => {
                return Ok(LoginScreenOutcome::Failed(message))
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(LoginScreenOutcome::Failed(
                    "Spotify login stopped unexpectedly".to_string(),
                ))
            }
        }

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => {
                        handle.cancel();
                        return Ok(LoginScreenOutcome::Cancelled);
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        handle.cancel();
                        return Ok(LoginScreenOutcome::Quit);
                    }
                    _ => {}
                }
            }
        }
    }
}
```

Also confirm `use crate::spotify;` (or equivalent path) — `play_loop.rs` may not import the spotify module yet; add `use crate::spotify;` if missing, and extend the `crate::tui::{...}` import list with `draw_spotify_login_prompt, draw_spotify_login_wait, SpotifyLoginScreenState`.

- [ ] **Step 2: Build + test**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -10`
Expected: clean build, all tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/play_loop.rs
git commit -m "feat(tui): in-app Spotify re-login that resumes the failed track"
```

---

### Task 6: Docs + full verification + manual smoke test

**Files:**
- Modify: `CLAUDE.md` (Spotify playback model section + TUI states list)

**Interfaces:** none (documentation + verification).

- [ ] **Step 1: Update `CLAUDE.md`**

In the "Spotify playback model" section, after the `src/spotify/mod.rs` bullet, add:

```markdown
- `src/spotify/login.rs` — the OAuth PKCE flow, driven directly with the
  `oauth2` crate (not `librespot-oauth`) so the TUI can display the auth URL,
  nothing writes to stdout, and Esc cancels the port-8898 redirect listener
  cleanly. `start_login()` runs on a background thread and streams
  `LoginPhase` over a channel; both the TUI recovery screen and
  `looper spotify login` consume it. A dead login at resolve time surfaces as
  a typed `SpotifyAuthError` (librespot connect errors with kind
  `PermissionDenied`/`Unauthenticated`, or no cached credentials), which the
  TUI turns into a "Spotify login needed" modal — `enter` re-logs-in in the
  browser and then re-resolves the originally requested URL.
```

In the TUI states list, after the "track unavailable" modal bullet, add:

```markdown
- "Spotify login needed" modal + login waiting screen (`draw_spotify_login_prompt`,
  `draw_spotify_login_wait`) — shown when resolve fails with a dead Spotify
  login. `enter` starts the in-TUI OAuth flow (browser + spinner + fallback
  auth URL, `esc` cancels), success resumes the original URL; any other key
  returns to the history browser. The generic "track unavailable" modal now
  shows the resolver's real error message instead of a hardcoded guess.
```

- [ ] **Step 2: Full verification**

Run: `cargo build && cargo test && cargo clippy 2>&1 | tail -20` (skip clippy if not installed)
Expected: build clean, all tests pass.

- [ ] **Step 3: Manual smoke test** (needs the user or a terminal with audio; document results honestly)

1. `mv ~/Library/Caches/looper/spotify/credentials.json ~/Library/Caches/looper/spotify/credentials.json.bak` (exact cache path: `crate::plugin::cache_dir_path()` — check `directories`' ProjectDirs output if this path differs).
2. `cargo run -- play --url https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC` → expect the "Spotify login needed" modal (detail: "not logged in to Spotify").
3. Press Esc on the waiting screen after `enter` → back at the modal; `enter` again → listener re-binds (no port error).
4. Complete the browser login → track resolves and plays without restarting.
5. Restore/verify: subsequent runs play without the modal (credentials re-cached).

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: describe Spotify re-login recovery flow"
```

---

## Self-Review Notes

- Spec §1 → Task 1; §2 → Task 3; §3 → Task 4 + Task 3 (real detail in generic modal); §4 → Tasks 2, 4, 5; §5 → Task 5; deps → Task 2; testing → Tasks 1, 2, 6.
- Type-consistency: `spotify::auth_error` (Tasks 1/3), `spotify::{start_login, LoginPhase, LoginHandle}` (Tasks 2/5), `SpotifyLoginScreenState`/draw fns (Tasks 4/5), `handle_unresolvable_replay(..., message: &str)` (Task 3) — names match across tasks.
- Task 3 ships a safe placeholder arm so the tree is green between Tasks 3 and 5.
