[← Back to README](../README.md)

# Cross-Device Sync

looper always reads and writes a fast local copy of `looper.sqlite3`. If you
point it at a cloud folder (iCloud Drive, Dropbox, anything that syncs files),
looper will pull from that folder at startup and push to it on quit. The cloud
folder never holds the live database — it's just a passive copy that the cloud
provider replicates between your machines on its own.

This avoids the long-standing footguns of running SQLite directly on a
cloud-synced filesystem (corrupted WAL/SHM sidecars, surprise permission
denials, evicted files).

## Where the live database lives

| Platform | Live database path |
|----------|--------------------|
| macOS | `~/Library/Application Support/sh.kbr.looper/looper.sqlite3` |
| Linux | `~/.local/share/looper/looper.sqlite3` |

By default no replication runs. Looper just uses the local path above.

## Replicate via a cloud folder

Point looper at any folder your cloud provider keeps in sync:

```shell
looper config set sync-folder "$HOME/Library/Mobile Documents/com~apple~CloudDocs/looper"
# Replication folder set to: ...
# looper will pull from this folder at startup and push to it on quit.
# The live DB stays at the platform data directory.
```

Run this once on each computer that should share history. The cloud provider
takes care of moving `looper.sqlite3` between machines in the background.

**Verify it's working:**

```shell
looper config show
# sync_folder = /Users/you/Library/Mobile Documents/.../looper (replicated on startup/quit)

ls "$HOME/Library/Mobile Documents/com~apple~CloudDocs/looper/"
# looper.sqlite3
```

## macOS: iCloud Drive needs Files-and-Folders permission

The first time looper tries to read or write inside `~/Library/Mobile Documents/...`,
macOS will silently deny access until you grant the terminal app that launches
looper permission. Open **System Settings → Privacy & Security → Files and
Folders** (or **Full Disk Access**) and toggle on **iCloud Drive** for your
terminal (Terminal, iTerm, Ghostty, etc.). Restart the terminal so the new
entitlement takes effect.

If permission isn't granted, looper still runs against the local DB, surfaces a
`History sync disabled` banner at startup, and prints a one-line warning to
stderr on quit. Nothing crashes.

## Sync semantics: last-quitter wins

Replication is a file copy in both directions:

- **At startup**: if the cloud copy has a more recent `MAX(last_played_at)` than
  the local copy, looper replaces local with the cloud copy.
- **At quit**: looper checkpoints the WAL, then atomically replaces the cloud
  copy with the local one.

This is enough for one-human-at-a-time use across multiple Macs (typical
single-user setup). It is **not** a general-purpose multi-master merge: if you
play on two machines simultaneously, whichever quits last overwrites the other's
session, and your cloud provider may produce conflict copies (e.g.
`looper.sqlite3 conflicted-copy 2`). Resolve by closing one, picking the version
you want to keep, and deleting the rest.

## Disable replication

```shell
rm "$HOME/.config/looper/sync_folder"
```

Or just don't set it. Looper falls back to local-only without complaint.
