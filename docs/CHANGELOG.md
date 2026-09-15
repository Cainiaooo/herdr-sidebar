# Fork changelog

This file records **this fork's** delta relative to upstream
[`alexarthurs/herdr-sidebar`](https://github.com/alexarthurs/herdr-sidebar).
It is not the upstream GitHub release notes.

How to maintain this fork: [`FORK.md`](FORK.md).

Base: upstream `1a5d37e` (v0.13.0, 2026-09-15).

---

## 2026-09-15 — merge upstream v0.13.0

Pulled Search view, host activity actions (`show-explorer` / `show-search` /
`show-git` / `quick-open`), Git footer/branch picker, image previews, and
Herdr 0.9 tab-focus preview fixes. Kept this fork's per-workspace visibility,
`prefix+shift+b` plugin toggle, configurable commit-message oneshot, and
graceful toggle-close.

Conflict resolutions:

- `ensure.rs`: take upstream `View` / `Activate` launch paths; keep
  `workspace_should_auto_open` instead of a global `auto_open` early-return;
  keep `persist_hide_after_close` for toggle, keep `request_close` for TUI.
- `launch.rs`: keep `event_pane_id` / `snooze_tab` / `workspace_id_from_scope`
  and take `event_scope_with_tab_context`.
- `explorer_app.rs` / `scm_app.rs`: hide still goes through `snooze::hide_pane`
  (workspace record + pane close). Settings keep both Git footer and Commit
  message rows.
- Unix `ensure-sidebar.sh` / `open-sidebar.sh` follow upstream deletion; native
  `--ensure` / `--toggle` now own those paths.
- `Cargo.lock` regenerated after the merge (`cargo build --release`).

---

## 2026-09-07 — merge upstream v0.11.0

Pulled Quick Open (`Ctrl+P`), light theme, and inline preview from
`alexarthurs/herdr-sidebar` v0.11.0. Kept this fork's per-workspace visibility,
`prefix+shift+b` plugin toggle, configurable commit-message oneshot, and the
SCM scroll snap so Staged/Changes stay on screen.

Conflict resolutions:

- `launch.rs`: keep `event_field` (pane id) and take upstream `event_scope_in`
  so `pane.focused` docks the event's own tab.
- `ensure-sidebar.sh`: still reads workspace-keyed auto-open before the lock,
  then re-reads scope from the pane snapshot after it.
- `CLAUDE.md`: keep both the scroll-snap gotcha and the Quick Open notes.
- `.github/workflows/release.yml`: follow upstream to `actions/upload-artifact@v7`
  (needs `gh auth refresh -s workflow` so the OAuth token can push workflow files).

---

## 2026-08-29 — configurable Agent CLI commit-message oneshot

Fork issue [#1](https://github.com/Cainiaooo/herdr-sidebar/issues/1). Sparkle (`✧` / `A`)
no longer hardcodes Anthropic Claude Code.

### Added

- User-level generator config: `%APPDATA%\herdr\plugins\config\herdr-sidebar\commit-message.toml`
  (unix: `$XDG_CONFIG_HOME/herdr/plugins/config/herdr-sidebar/`, else `~/.config/...`).
  Named profiles, argv `command` (no shell), input modes (`prompt_argv_diff_stdin`,
  `prompt_and_diff_stdin`, `argv_subst`), prompt placeholders, timeout/size caps,
  `auto_on_empty_commit` (`off` / `fill` / `fill_and_commit`).
- Non-loaded example: [`docs/examples/commit-message.toml`](examples/commit-message.toml).
  Copying it into the user config dir is the only way it takes effect.
- ⚙ Settings row **Commit message**: read-only summary of the resolved profile
  (`built-in: claude haiku` when no file). Edit the TOML to switch profiles.

### Changed

- No config file → same as today: `claude -p --model haiku --strict-mcp-config` plus
  the existing English prompt, filename fallback, empty Commit is a no-op.
- `state::spawn_env()` forwards `HERDR_PLUGIN_CONFIG_DIR` (herdr injects it for
  hooks/actions, not panes) and falls back to the conventional user config dir.
- Generator child inherits the user environment (so Claude/Codex/Grok auth still
  works) and strips `HERDR_*` control variables before spawn.
- Invalid / oversize / unknown-placeholder config fails closed: flash the error,
  use the filename fallback, do not run a guessed binary.

### Files

| Path | Role |
|---|---|
| `plugins/herdr-sidebar/src/suggest.rs` | load config, plan argv/stdin, spawn, parse |
| `plugins/herdr-sidebar/src/state.rs` | `plugin_config_dir()`, forward in `spawn_env()` |
| `plugins/herdr-sidebar/src/scm_app.rs` | empty-commit fill; Settings summary |
| `plugins/herdr-sidebar/src/git.rs` | `MessageDiff` / `{source}` |
| `docs/examples/commit-message.toml` | non-loaded example |

---

## 2026-08-28 — review fixes (unix list, hide-after-close, install path)

Codex review of the visibility patch (`upstream/main...HEAD`).

### Fixed

- Unix ensure/toggle called `herdr workspace list --json`. Herdr 0.8.2 rejects
  `--json` (exit 2); empty stdin made `--should-auto-open` / `--remember-sidebar`
  fall back to the transient workspace id, so label-keyed hides from the TUI
  were ignored. Use `herdr workspace list` (already JSON), matching `redeploy.sh`.
- Toggle close recorded the workspace as hidden even when `graceful_close` was
  cancelled (dirty editor). Windows sidecar and unix `open-sidebar.sh` now snooze
  and persist `false` only after the pane acknowledges quit.
- Unix ensure exited on `should=off` *before* `launch-decision`, so a label-only
  corpse in a hidden workspace was never REPLACE-closed. It now closes corpses
  first, then skips redocking — same as the Windows sidecar.
- Fork README install snippets pointed at `alexarthurs/herdr-sidebar`. They now
  install `Cainiaooo/herdr-sidebar` and name the public repo as upstream-only.

---

## 2026-08-28 — per-workspace sidebar visibility

**Commits:** `995ab07`, merge `5dca988`.

### Why

The public plugin auto-docks a sidebar into every herdr workspace. Hiding with
`b` / `«` only wrote a **tab** snooze file under `%TEMP%`. herdr's `pane.focused`
payload has `pane_id` + `workspace_id` and **no** `tab_id`, so clicking another
pane in the same tab (Perforce, a shell) made the ensure hook treat the event as
workspace-scoped, skip snooze, and dock again.

Herdr's built-in `prefix+b` (`Ctrl+B` then `B`) is `keys.toggle_sidebar` — Herdr's
own workspace/agent chrome — not this plugin. Binding the plugin to that chord
steals the native sidebar.

### Added

- Durable per-workspace show/hide in
  `%LOCALAPPDATA%\herdr\plugins\herdr-sidebar\visibility.json` (unix: the plugin
  state dir). Keyed by **workspace label**, same reason as `roots.json` (ids are
  instance handles and get reassigned).
- Explicit **toggle open** (plugin action / bound key) records `true` for that
  space. Later tab/pane focus auto-docks there even if global Auto-open is off.
- **Hide** (`b` / `«`) and **toggle close** record `false`. Later focus in that
  space does not auto-dock, including a click on Perforce or a shell.
- Quiet ensure hooks **never** write this file. Auto-docking an unknown space is
  not a user choice.
- ⚙ Settings row **Auto-open (new spaces)** is only the default for workspaces
  with no recorded preference.
- CLI helpers for the unix launchers: `--should-auto-open`, `--remember-sidebar`,
  `--focused-workspace`, `--snooze-tab`.

### Changed

- `pane.focused` now resolves the focused pane's **tab** for snooze, instead of
  borrowing nothing (or the wrong space) from a workspace-only payload.
- Recommended herdr bind is `prefix+shift+b` →
  `herdr plugin action invoke open-sidebar-windows --plugin herdr-sidebar`
  (unix: `open-sidebar`). Do not steal `prefix+b`.
- Shared hide path is `snooze::hide_pane` (Explorer and Source Control).

### Files

| Path | Role |
|---|---|
| `plugins/herdr-sidebar/src/state.rs` | `visibility.json` load/save, `should_auto_open_workspace` |
| `plugins/herdr-sidebar/src/launch.rs` | `event_pane_id`, `snooze_tab`, `workspace_id_from_scope` |
| `plugins/herdr-sidebar/src/ensure.rs` | Windows sidecar: honor record; remember on toggle |
| `plugins/herdr-sidebar/src/snooze.rs` | `hide_pane` writes snooze + visibility off |
| `plugins/herdr-sidebar/src/main.rs` | unix CLI flags |
| `plugins/herdr-sidebar/scripts/ensure-sidebar.sh` | unix ensure uses `--should-auto-open` / `--snooze-tab` |
| `plugins/herdr-sidebar/scripts/open-sidebar.sh` | unix toggle writes the workspace record |

### Not in this fork (still upstream-only)

- No per-workspace preference on a stock `herdr plugin install alexarthurs/...`
  binary. Global Auto-open off + a custom keybind is the only public-version
  workaround, and hide still comes back on `pane.focused`.
