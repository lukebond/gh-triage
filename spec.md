# gh-triage

A GitHub notification triage tool for developers who want to stay on top of things without missing anything. Written in Rust.

## Overview

`gh-triage` polls GitHub for activity you care about, maintains local state in SQLite, fires desktop notifications via `notify-send`, shows a Waybar-compatible badge, and presents a TUI for browsing and actioning items.

It does **not** use the GitHub notifications API. Instead it uses the GitHub search API to query directly for items relevant to you, giving full control over what is tracked.

## Binary & Config Locations

- Binary: `gh-triage` (installed to `~/.cargo/bin/` via `cargo install`)
- Config: `~/.config/gh-triage/config.toml`
- Database: `~/.local/share/gh-triage/db.sqlite`
- Create both directories on first run if they don't exist

## Config File (`config.toml`)

```toml
# GitHub personal access token (or use GH_TOKEN env var)
github_token = "ghp_..."

# Your GitHub username
github_user = "lukec"

# The organisation to monitor
github_org = "restatedev"

# Poll interval in seconds (default: 120)
poll_interval = 120

# What "enter" does in the TUI: "browser" or "preview"
# "browser" opens the item URL in the default browser
# "preview" shows full markdown detail in the TUI (not implemented in v1, fall back to browser)
enter_action = "browser"

# notify-send urgency for new items: "low", "normal", "critical" (default: "normal")
notify_urgency = "normal"

# Notify on: "new_item" (only when first seen) or "new_activity" (any new update to tracked item)
# default: "new_activity"
notify_on = "new_activity"

# Repos listed under [repos.all] get everything — all issues and PRs.
# Repos listed under [repos.ignore] are skipped entirely.
# All other repos in the org use the "for_me" preset (default).
# A repo should appear in at most one section.
[repos.all]
repos = [
    "restatedev/restate",
]

[repos.ignore]
repos = [
    "restatedev/old-thing",
]
```

### Presets explained

- **`for_me`** (default for all org repos not listed above): queries for PRs/issues where you are a requested reviewer, assignee, or are mentioned. Specifically:
  - `is:open review-requested:@me org:{org}`
  - `is:open assignee:@me org:{org}`
  - `is:open mentions:@me org:{org}`
  - Also recently closed items matching the above (so you see resolution)
- **`all`**: everything in that specific repo — all issues and PRs
- **`ignore`**: skip this repo entirely

## GitHub API

Use the GitHub REST search API:
- Base URL: `https://api.github.com`
- Auth: `Authorization: Bearer {token}` header
- Use `reqwest` (async, with `tokio` runtime)
- Respect rate limits: check `X-RateLimit-Remaining` header, back off if low
- Search endpoint: `GET /search/issues?q={query}&per_page=100`

### Queries to run on each poll

For the "for me" preset (run these 3, deduplicate by item URL):
```
review-requested:@me org:{org} is:open
assignee:@me org:{org} is:open
mentions:@me org:{org} is:open
```

Also run with `is:closed updated:>{last_poll_time}` to catch recently resolved items.

For each `all` repo:
```
repo:{org}/{repo}
```

Merge all results, deduplicate by `html_url`.

## Local State (SQLite via `rusqlite`)

Use `rusqlite` with the `bundled` feature so SQLite is statically linked — no external dependency.

### Schema

```sql
CREATE TABLE IF NOT EXISTS items (
    id TEXT PRIMARY KEY,          -- GitHub node_id
    url TEXT NOT NULL,            -- html_url
    repo TEXT NOT NULL,           -- e.g. "restatedev/restate-cloud"
    title TEXT NOT NULL,
    body TEXT,                    -- issue/PR body markdown
    item_type TEXT NOT NULL,      -- "issue" or "pull_request"
    state TEXT NOT NULL,          -- "open" or "closed"
    reason TEXT,                  -- "review_requested", "assigned", "mentioned", "all"
    author TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,     -- from GitHub, used to detect new activity
    first_seen_at TEXT NOT NULL,  -- local timestamp, when we first fetched it
    last_activity_at TEXT,        -- most recent update_at we've seen, for detecting new activity
    summary TEXT,                 -- AI summary, generated once, stored here
    status TEXT NOT NULL DEFAULT 'active'  -- "active", "archived"
);

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Store last poll time: INSERT OR REPLACE INTO meta VALUES ('last_poll', '{iso8601}');
```

## Polling Daemon (`gh-triage daemon`)

Run as a long-lived process (managed by the user via systemd user service or just a terminal).

On each poll cycle:
1. Fetch items from GitHub search API per config
2. For each item:
   - If not in DB: insert it, generate AI summary (async, non-blocking), fire `notify-send`
   - If in DB and `updated_at` changed: update record, fire `notify-send` if `notify_on = "new_activity"`
   - If in DB and unchanged: do nothing
3. Update `last_poll` in meta table
4. Sleep for `poll_interval` seconds

### AI Summary generation

Shell out to `claude`:
```bash
claude -p "{prompt}" --no-session-persistence --allowedTools "" --output-format text
```

Prompt template:
```
Summarise this GitHub {type} in 1-2 sentences. Be concise and focus on what action (if any) is needed from a reviewer or assignee.

Title: {title}
Body: {body_truncated_to_2000_chars}
```

Run this in a background task (tokio::spawn) so it doesn't block the poll cycle. Store result in `summary` column when done. If `claude` is not found in PATH or fails, store `None` and don't retry (leave summary blank in TUI).

### notify-send

```bash
notify-send -u {urgency} "GitHub: {repo}" "{title}"
```

If `notify-send` is not found, log a warning and continue.

## TUI (`gh-triage` or `gh-triage tui`)

Use `ratatui` with `crossterm` backend.

### Layout

```
┌─────────────────────────────────────────────────────────────┐
│ gh-triage          [active: 12]    org: restatedev    q quit│
├─────────────────────────────────────────────────────────────┤
│ [latest] [by repo]                          [a archived]    │
├─────────────────────────────────────────────────────────────┤
│ ● PR  restate-cloud   Review requested   Add PSC support    │
│   Needs your review • opened by alice • 2h ago              │
│                                                             │
│ ● ISS restate         Assigned          Memory leak in...   │
│   Fix this soon • opened by bob • 1d ago                    │
│                                                             │
│   PR  restate-cloud   Mentioned         Update docs for..   │
│   No action needed • opened by carol • 3d ago               │
└─────────────────────────────────────────────────────────────┘
```

### Item display

Each item takes 2 lines:
- Line 1: `[●/space] [PR/ISS] [repo]  [reason]  [title truncated]`
  - `●` = new/unseen since last TUI open; space = previously viewed in TUI
  - Reason: "Review requested", "Assigned", "Mentioned", "All"
- Line 2: `  [AI summary if available, else "Fetching summary..."]  • [author]  • [relative time]`

Colour coding:
- "Review requested" → yellow
- "Assigned" → cyan  
- "Mentioned" → white
- Unseen indicator `●` → bright red

### Views

- **Latest** (default): all active items sorted by `updated_at` desc, newest first
- **By repo**: grouped by repo name, within each group sorted by `updated_at` desc

Toggle with `Tab`.

### Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Open in browser (or preview, per config) |
| `a` | Archive item (sets status = 'archived', removes from active list) |
| `A` | View archived items (toggle) |
| `R` | Refresh now (trigger a poll immediately) |
| `Tab` | Toggle latest / by repo view |
| `q` | Quit |

### "Viewed in TUI" state

When an item is scrolled into the selected position in the TUI, mark it as "seen in TUI" in memory only — not persisted to the DB, resets on next launch. The `●` indicator clears for the session. There is no read/unread distinction persisted to the DB or synced to GitHub. The only persisted states are `active` and `archived`. An item stays in the active list until explicitly archived with `a`.

## Waybar Integration (`gh-triage waybar`)

Output a single line of JSON to stdout and exit:

```json
{"text": " 3", "tooltip": "3 active GitHub notifications\nrestate-cloud: Add PSC support\nrestate: Memory leak...", "class": "has-notifications"}
```

- Count = number of items with `status = 'active'`
- Tooltip lists up to 5 item titles
- If count is 0: `{"text": "", "tooltip": "No notifications", "class": ""}`
- Use a nerd font icon (`` = bell, or `` = octocat-ish)

Waybar config example (include in README):
```json
"custom/gh-triage": {
    "exec": "gh-triage waybar",
    "interval": 60,
    "return-type": "json",
    "on-click": "your-terminal -e gh-triage"
}
```

## Subcommands Summary

| Command | Description |
|---------|-------------|
| `gh-triage` | Launch TUI (default) |
| `gh-triage daemon` | Run polling daemon |
| `gh-triage waybar` | Print Waybar JSON and exit |
| `gh-triage poll` | Run one poll cycle and exit |
| `gh-triage list` | Print active items to stdout (no TUI) |
| `gh-triage archive <id>` | Archive an item by ID |

## Dependencies (`Cargo.toml`)

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
rusqlite = { version = "0.31", features = ["bundled"] }
ratatui = "0.28"
crossterm = "0.28"
chrono = { version = "0.4", features = ["serde"] }
dirs = "5"
thiserror = "1"
clap = { version = "4", features = ["derive"] }
open = "5"
```

## Error Handling

Use `thiserror` throughout for all error types. Define a typed error enum per module (e.g. `ConfigError`, `DbError`, `GithubError`, `SummaryError`) in each module, and a top-level `AppError` in `main.rs` that wraps them. Do not use `anyhow`. Do not use `unwrap()` or `expect()` outside of tests.

## Systemd User Service (include template in README)

```ini
[Unit]
Description=gh-triage GitHub notification daemon
After=network-online.target

[Service]
ExecStart=%h/.cargo/bin/gh-triage daemon
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
```

Install: `~/.config/systemd/user/gh-triage.service`

## Code Quality

- **No `unwrap()` or `expect()`** outside of tests — all errors must be propagated via `?` or handled explicitly
- **No repetition** — shared types (GitHub API response structs, DB item structs, config types) live in `src/types.rs` and are imported where needed; no duplicated struct definitions
- **Clippy**: code must pass `cargo clippy -- -D warnings` with no warnings
- **Formatting**: code must pass `cargo fmt --check`
- **Tests**: unit tests for the following, kept in `#[cfg(test)]` modules within each file:
  - `config.rs`: parsing a valid config, missing optional fields get defaults, unknown repo defaults to `for_me`
  - `db.rs`: insert item, fetch active items, archive item, duplicate insert is a no-op or update
  - `github.rs`: search query construction for each preset (`for_me`, `all`), deduplication of results by URL
  - `waybar.rs`: correct JSON output with items, correct JSON output with zero items
- Do not write integration tests or end-to-end tests in v1
- Tests may use `unwrap()` freely

## v1 Scope — Explicitly Out of Scope

- Filtering/searching within TUI (future)
- Preview mode in TUI (future — just open browser for now)
- Multiple orgs
- Snooze/remind later
- Any web UI
