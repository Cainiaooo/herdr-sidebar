# Fork changelog

This file records **this fork's** delta relative to upstream
[`alexarthurs/herdr-sidebar`](https://github.com/alexarthurs/herdr-sidebar).
It is not the upstream GitHub release notes.

How to maintain this fork: [`FORK.md`](FORK.md).

Base: upstream `13ebde8` (after PR #39, 2026-08).

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
