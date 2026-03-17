# gh-triage

A GitHub notification triage tool for developers who want to stay on top of things without missing anything. Written in Rust.

`gh-triage` polls GitHub for activity you care about, maintains local state in SQLite, fires desktop notifications via `notify-send`, shows a Waybar-compatible badge, and presents a TUI for browsing and actioning items.

It does **not** use the GitHub notifications API. Instead it uses the GitHub search API to query directly for items relevant to you, giving full control over what is tracked.

## Install

```bash
cargo install --path .
```

A sample config is included in the repo — copy it and fill in your details:

```bash
mkdir -p ~/.config/gh-triage
cp config.example.toml ~/.config/gh-triage/config.toml
$EDITOR ~/.config/gh-triage/config.toml
```

## Config

See `config.example.toml` for a commented starting point. The key fields:

```toml
github_token = "ghp_..."   # or use GH_TOKEN env var
github_user = "your-username"
github_org = "your-org"
```

### Presets

| Preset | Behaviour |
|--------|-----------|
| `for_me` (default) | PRs/issues where you are a requested reviewer, assignee, or mentioned |
| `all` | Everything in the repo — all issues and PRs |
| `ignore` | Skip the repo entirely |

## Usage

| Command | Description |
|---------|-------------|
| `gh-triage` | Launch TUI (default) |
| `gh-triage daemon` | Run polling daemon |
| `gh-triage waybar` | Print Waybar JSON and exit |
| `gh-triage poll` | Run one poll cycle and exit |
| `gh-triage list` | Print active items to stdout |
| `gh-triage archive <id>` | Archive an item by ID |
| `gh-triage setup systemd` | Install systemd user service |
| `gh-triage setup waybar` | Print Waybar config snippet |

## TUI keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Open in browser |
| `a` | Archive item |
| `A` | Toggle archived view |
| `Tab` | Toggle latest / by-repo view |
| `R` | Refresh |
| `q` | Quit |

Items show a `●` indicator when unseen in the current session. Colour coding: yellow = review requested, cyan = assigned, white = mentioned.

## AI summaries

When new items are found, `gh-triage` shells out to the `claude` CLI to generate a 1-2 sentence summary. If `claude` is not in PATH, summaries are silently skipped.

## Setup helpers

### Systemd service

Install the daemon as a systemd user service:

```bash
gh-triage setup systemd
```

This writes `~/.config/systemd/user/gh-triage.service` and prints the enable command. It won't overwrite an existing file.

### Waybar

Print the Waybar config snippet:

```bash
gh-triage setup waybar
gh-triage setup waybar --terminal foot   # override terminal (default: alacritty)
```

Paste the output into your Waybar config and reload.

### Shell greeting

To see a quick summary of active items each time you open a terminal, add this to your `~/.bashrc` (or `~/.zshrc`):

```bash
gh-triage list 2>/dev/null
```

Or for a one-line count:

```bash
echo "$(gh-triage waybar 2>/dev/null | jq -r '.tooltip // empty')"
```

Either line will silently do nothing if the database doesn't exist yet or the config isn't set up.

## Data locations

| Path | Purpose |
|------|---------|
| `~/.config/gh-triage/config.toml` | Configuration |
| `~/.local/share/gh-triage/db.sqlite` | Local state |
| `~/.config/systemd/user/gh-triage.service` | Daemon service (optional) |
