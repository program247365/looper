# Spotify Re-Login Recovery Flow

## Problem

When looper's cached Spotify credentials expire or are revoked, playing any
Spotify item fails with the generic "track unavailable" modal ("This track may
be private, removed, or region-locked"). The real cause — the login is no
longer valid — is never shown, and the only path out is quitting the TUI and
running `looper spotify login` by hand. Two defects compound this:

1. `connect_session` (`src/spotify/mod.rs`) produces a clear auth error, but
   the resolver thread flattens it to a `String`, destroying the type.
2. `play_file_session` (`src/play_loop.rs`) discards even that string
   (`ResolveStartupOutcome::Failed(_message)`) and shows a hardcoded guess.

## Goal

Detect an expired/revoked Spotify login distinctly, tell the user what
happened, offer an in-TUI re-login (browser OAuth), and on success resume
playing the item that originally failed — no dead end, no manual CLI step.

## Design

### 1. Typed auth-failure classification (`src/spotify/mod.rs`)

- New `SpotifyAuthError(String)` implementing `std::error::Error`, attached to
  the eyre chain by `connect_session` when:
  - the cache has no credentials ("not logged in"), or
  - `Session::connect` fails with `librespot_core::Error` kind
    `PermissionDenied` or `Unauthenticated` (librespot maps `LoginFailed` and
    other auth failures to these kinds).
- Network/other failures keep their current wording and stay untyped.
- `pub fn is_auth_error(err: &Report) -> bool` walks `err.chain()` looking for
  `SpotifyAuthError`.

### 2. Classification survives the resolver thread (`src/play_loop.rs`)

- The resolver thread in `resolve_url_with_startup` reports
  `Result<Option<Vec<TrackInfo>>, ResolveFailure>` where
  `ResolveFailure::AuthRequired(String) | Other(String)`, classified via
  `is_auth_error` before crossing the channel.
- `ResolveStartupOutcome` gains `SpotifyLoginRequired(String)`.
- The existing `Failed(message)` path now threads the real message into the
  generic modal as its detail line instead of the hardcoded guess (keeping the
  guess as a fallback framing where the message is unhelpful is not required —
  show the message).

### 3. "Spotify login needed" modal (`src/tui.rs`)

- `draw_spotify_login_prompt`, styled like `draw_replay_error`.
- Copy: title "Spotify login needed"; body "Your Spotify login has expired or
  been revoked." plus the error detail.
- Keys: `enter` = log in again; `q`/Ctrl-C = quit; any other key = back to the
  history browser (matches the existing modal's convention).

### 4. In-TUI OAuth login (`src/spotify/login.rs`, new)

One code path shared by the TUI and `looper spotify login`:

- Drive the `oauth2` crate directly (PKCE authorization-code flow against
  `accounts.spotify.com`, redirect `http://127.0.0.1:8898/login`, scope
  `streaming`) instead of `librespot-oauth`'s blocking wrapper. This makes the
  auth URL programmatically available, avoids stray `println!`s under the TUI,
  and lets us own listener lifetime for clean cancellation.
- Flow (background thread, phases over an `mpsc` channel):
  1. Bind `TcpListener` on 8898 (non-blocking), generate auth URL.
  2. Send `WaitingForBrowser { auth_url }`; open the browser via the `open`
     crate.
  3. Poll accept + cancel flag (`AtomicBool`); on redirect, parse the `code`,
     respond with the "go back to your terminal" page.
  4. Send `Connecting`; exchange code for token; `Session::new` +
     `connect(Credentials::with_access_token(..), store_credentials: true)`
     inside the shared runtime; install the fresh session into the shared
     session slot.
  5. Send `Done(Result<username, message>)`.
- Cancel (Esc): set the flag; the thread drops the listener (port freed) and
  exits; a retry can re-bind.
- TUI login screen: spinner + "Finish logging in to Spotify in your
  browser…", the auth URL shown as fallback, `esc` cancels back to the modal.
- CLI `looper spotify login` reuses the same flow, printing the URL and
  result to stdout. Direct use of `librespot-oauth` goes away.

### 5. Resume playback

- `Done(Ok)` → `play_file_session`'s loop `continue`s: the original
  `current_url` re-resolves against the fresh session and plays.
- `Done(Err)` / cancel → back to the modal (with the failure detail on error);
  `enter` retries.

## Scope

- Covers every rail that funnels through `resolve_url_with_startup`: direct
  URL launch, history-browser replay, search-overlay Enter.
- Out of scope: mid-playlist auth death (already self-heals at track
  boundaries when credentials are valid; now at least exits with a clear
  message when they aren't), Web-API search credentials (separate token
  system), auto-refresh of credentials before expiry.

## Dependencies

- `oauth2` and `open` become direct dependencies, pinned to the versions
  already in `Cargo.lock` via librespot — no new compiled code.
- `librespot-oauth` remains in the tree (librespot-core depends on it) but
  looper stops calling it.

## Testing

- Unit tests: auth-error classification (kind → `SpotifyAuthError`; missing
  credentials → `SpotifyAuthError`; other kinds → not), redirect-request code
  parsing, `ResolveFailure` classification.
- Manual smoke test: expire credentials (move the Spotify cache credentials
  file aside), `looper play --url <spotify track>`, verify modal → enter →
  browser → resume; verify Esc cancel and immediate retry; verify non-auth
  failures still show the generic modal with real detail.
