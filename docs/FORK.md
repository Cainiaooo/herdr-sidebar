# This fork

This checkout is **Cainiaooo's fork** of
[alexarthurs/herdr-sidebar](https://github.com/alexarthurs/herdr-sidebar).

We keep our own behavior (per-workspace sidebar visibility, plugin toggle that
does not steal Herdr's `prefix+b`) and pull upstream when they ship something
we want. We do **not** push to the public repo.

What we changed vs upstream: [`CHANGELOG.md`](CHANGELOG.md).

## Remotes

| Remote | URL | Push? |
|---|---|---|
| `origin` | https://github.com/Cainiaooo/herdr-sidebar.git | yes — this is our repo |
| `upstream` | https://github.com/alexarthurs/herdr-sidebar.git | **never** |

```
git remote -v
```

If `origin` still points at `alexarthurs/herdr-sidebar`, fix it before any
`git push`:

```
git remote rename origin upstream
git remote add origin https://github.com/Cainiaooo/herdr-sidebar.git
git fetch origin
git branch -u origin/main main
```

## How herdr uses this tree

herdr is **linked to this checkout**, not to the GitHub prebuilt of the public
plugin:

```
plugin_root = D:\Projects\herdr-sidebar\plugins\herdr-sidebar
source      = local
```

Do **not** run `herdr plugin install alexarthurs/herdr-sidebar/plugins/herdr-sidebar`.
That replaces the link with the public binary and drops our patches.

If the link is lost:

```
cd plugins/herdr-sidebar
cargo build --release
herdr plugin link .
herdr plugin action invoke herdr-sidebar.redeploy-windows
```

A machine that should run the fork without this checkout:

```
herdr plugin install Cainiaooo/herdr-sidebar/plugins/herdr-sidebar --yes
```

(`--yes` is required when stdin is not a TTY.) That still builds from **our**
GitHub `main`, not alexarthurs.

## Day-to-day (this machine)

Edit on `main` (or a feature branch you merge locally). Then:

```
git push origin main
cd plugins/herdr-sidebar
cargo build --release
herdr plugin action invoke herdr-sidebar.redeploy-windows
```

Windows locks a running `herdr-sidebar.exe` (`os error 5`). Close the sidebar
panes first, or rename the running exe aside (`herdr-sidebar-old.exe`), build,
then redeploy. See `CLAUDE.md`.

## Toggle the plugin sidebar

Herdr's `Ctrl+B` then `B` is **Herdr's own** sidebar (`keys.toggle_sidebar`).

This machine binds the plugin to **`Ctrl+B` then `Shift+B`** in
`%APPDATA%\herdr\config.toml`:

```toml
[[keys.command]]
key = "prefix+shift+b"
type = "shell"
command = "herdr plugin action invoke open-sidebar-windows --plugin herdr-sidebar"
description = "toggle herdr-sidebar"
```

After editing that file: `herdr server reload-config`.

From any pane:

```
herdr plugin action invoke herdr-sidebar.open-sidebar-windows
```

### Per-workspace memory

| Action | Remembered for this workspace |
|---|---|
| Plugin toggle **open** | show — later visits auto-dock |
| `b` / `«` hide, or toggle **close** | hide — later visits stay closed, including a click on Perforce |

File: `%LOCALAPPDATA%\herdr\plugins\herdr-sidebar\visibility.json`, keyed by
workspace **label**. ⚙ **Auto-open (new spaces)** only applies to spaces with
no row in that file.

Git space: open once with the plugin toggle. Perforce space: hide once with
`b`.

## Sync upstream

```
git fetch upstream
git merge upstream/main
# resolve conflicts; our delta is listed in CHANGELOG.md
git push origin main
cd plugins/herdr-sidebar
cargo build --release
herdr plugin action invoke herdr-sidebar.redeploy-windows
```

Never `git push upstream`. A `git pull` with no remote name follows `origin`
(the fork).

`gh auth git-credential` is an OAuth app. Its default scopes (`repo`, `gist`,
`read:org`) **cannot push changes under `.github/workflows/`**. If upstream
touched a workflow, the whole `git push origin main` is rejected even though
the rest of the merge is fine:

```
refusing to allow an OAuth App to create or update workflow
`.github/workflows/release.yml` without `workflow` scope
```

Fix: `gh auth refresh -s workflow` (browser device flow), then push. Do not use
GitHub's **Sync fork** button for this — it hits the same restriction. Once the
token has `workflow`, follow upstream workflow files (currently
`actions/upload-artifact@v7`).

If a merge looks wrong, compare against the last known-good upstream base
recorded in `CHANGELOG.md` (currently v0.13.0 / `1a5d37e`).

## Reviewer map

| Area | Start here |
|---|---|
| Visibility persist | `plugins/herdr-sidebar/src/state.rs` (`visibility.json`) |
| `pane.focused` tab/workspace | `plugins/herdr-sidebar/src/launch.rs` |
| Windows ensure/toggle | `plugins/herdr-sidebar/src/ensure.rs` |
| Hide (`b`) | `plugins/herdr-sidebar/src/snooze.rs` `hide_pane` |
| Unix ensure/toggle | `scripts/ensure-sidebar.sh`, `scripts/open-sidebar.sh` |
| ✧ commit-message generator | `plugins/herdr-sidebar/src/suggest.rs`; config `%APPDATA%\herdr\plugins\config\herdr-sidebar\commit-message.toml` (never `state.json`, never the repo) |
| Tests | `state::workspace_visibility_*`, `launch::pane_focused_events_*`, `suggest::*` |
