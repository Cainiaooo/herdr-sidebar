> **Fork.** This tree is [Cainiaooo/herdr-sidebar](https://github.com/Cainiaooo/herdr-sidebar),
> not the public [alexarthurs/herdr-sidebar](https://github.com/alexarthurs/herdr-sidebar)
> install. How we use and sync it: [`docs/FORK.md`](docs/FORK.md). What we changed:
> [`docs/CHANGELOG.md`](docs/CHANGELOG.md). Do not `git push` to upstream; do not
> `herdr plugin install alexarthurs/...` on this machine.

<div align="center">

# Herdr Sidebar

### The sidebar your terminal was missing — inspired by VS Code.

A file explorer and a full source-control panel in one dockable
[herdr](https://github.com/ogulcancelik/herdr) pane — activity-bar switching, mouse
everywhere, AI-drafted commit messages, and file previews that open as editor tabs —
ephemeral until you double-click to pin one.

<img alt="Rust" src="https://img.shields.io/badge/Rust-self--contained_crate-orange?logo=rust&logoColor=white">
<img alt="herdr" src="https://img.shields.io/badge/herdr-%E2%89%A5%200.8-5865a3">
<img alt="Platforms" src="https://img.shields.io/badge/Windows%20%C2%B7%20macOS%20%C2%B7%20Linux-supported-2ea44f">
<img alt="CI" src="https://github.com/alexarthurs/herdr-sidebar/actions/workflows/ci.yml/badge.svg">
<img alt="License" src="https://img.shields.io/badge/license-MIT-blue">

<br><br>

<img src="plugins/herdr-sidebar/docs/media/hero.png" alt="The sidebar docked beside a 2x2 fleet of Claude Code and Codex agents" width="920">

</div>

That's the sidebar on the left and a 2×2 fleet of Claude Code and Codex agents beside it —
the workflow herdr is built for. If you've ever alt-tabbed out of your terminal just to *look*
at something — the tree, the diff, what's staged — this closes that loop. The sidebar docks
on either edge of every herdr tab (left by default), restores itself on focus, and is driven
entirely by click or keystroke.

```
herdr plugin install Cainiaooo/herdr-sidebar/plugins/herdr-sidebar --yes
```

That is **this fork**. The public upstream remains `alexarthurs/herdr-sidebar/plugins/herdr-sidebar` and does not include the per-workspace hide/show record. `--yes` is required when stdin is not a TTY.

Tagged releases install SHA-256-verified prebuilt binaries on supported Windows, macOS,
and Linux systems. Unsupported targets or unavailable assets fall back to a source build.

---

## One pane. Two views. Zero friction.

The activity bar at the top flips between **Explorer** and **Source Control** — *in
process*, so switching is instant: no respawn, no flicker, no lost state on the way. Both
views ship in one small Rust binary.

### 🗂 The Explorer

A real tree, not a directory dump:

<div align="center">
<img src="plugins/herdr-sidebar/docs/media/preview.png" alt="Explorer and live file preview interface" width="920">
</div>

- Disclosure chevrons, nested indentation, and **two icon themes** — colored Nerd Font
  glyphs (Atom-Material style) or emoji, toggled live. The sidebar auto-picks: material
  when a Nerd Font is installed, emoji otherwise — and on first run without one it
  offers to download and install JetBrainsMono Nerd Font for you (Windows, macOS,
  Linux). If the theme ever guesses wrong (icons showing as ⌷ tofu boxes), press `i`
  once; the choice persists.
- **Click a file and it opens in its own tab** — with the sidebar docked alongside it,
  mirroring the tree you clicked from. Your terminals are never moved. The tab is
  *ephemeral* (labelled `name · preview`), so clicking another file reuses it; **double-click**
  to pin it and the next file gets a fresh tab. Click a file that is already open and
  you jump to its tab instead of opening it twice. Closing a preview returns to the tab
  that opened it. Line numbers, scrolling,
  binary-safe, and **long lines wrap** — press `w` to switch wrapping off for the
  document you are reading.
- **Experimental in-pane editing** — press `e` in a regular UTF-8 file preview. The editor
  has a visible cursor, word-aware wrapping, wrapped-row scrolling, click/drag selection, find, and
  best-effort system clipboard integration. Saving is always explicit; unsaved exits and
  file switches ask first, and an external disk change can never be overwritten silently. The
  first actual edit pins the tab automatically, so later file clicks open a fresh preview tab.
  Diff and history previews remain read-only.
- **Follows the pane's live folder by default** — `cd` in a neighbouring shell (or point
  an agent at another project) and the tree and Source Control re-root within ~5s. A
  folder you choose manually stays selected until an already-seen pane actually changes
  folder; turn "Follow pane folder" off in ⚙ Settings for a fixed root.
- **Git status, right in the tree** — modified, added, deleted, untracked, conflicted
  and ignored files carry the same status letters and colors the Source Control view
  uses, and a folder shows a **dirty dot** when anything inside it changed, so you spot
  work without expanding a thing. Decorations refresh on their own every couple of
  seconds and immediately after you stage. Nested repositories decorate independently —
  an inner checkout's changes never leak into the parent's folders. Not for you? Turn
  "Git decorations" off in ⚙ Settings.
- **Stage from the tree** — "Stage Changes" in the context menu runs the `git add` for a
  file or a whole folder (additions, modifications *and* deletions), against the repo
  that actually owns the path. Staging a parent folder stops dead at a nested repository
  boundary, and staging something inside one stages it *there*.
- **Double-click folders** to fold, hover highlights, mouse wheel, and a context menu on
  **`m`** or **Ctrl+right-click** — no mouse required, which is what makes it work on
  mobile herdr clients: New File, New Folder, Open with Default App (files only — hands
  the file to the OS-associated app, like a double click in the file manager), Stage
  Changes, Rename, Delete, Copy Path / Relative Path, Reveal in File Explorer.
- Dotfiles toggle, live refresh, and a hide command when you want the columns back.
- Prefer the sidebar closed in some spaces (e.g. Perforce) and open in others (Git)?
  Hide with `b` / `«` or toggle it closed — that workspace is remembered off.
  Toggle it open and that workspace is remembered on. ⚙ "Auto-open (new spaces)" is
  only the default for workspaces you have not shown or hidden yet.
- Want one key to mean open/close? "Strict toggle" in ⚙ Settings closes an open sidebar
  even when it isn't focused, and "Focus on open" off docks it in the background so
  focus stays in the pane you toggled from.
- Prefer the tree on the other side? Toggle "Dock on the right" in ⚙ Settings; the choice
  persists for every future launch and auto-docked tab.
- Prefer a different width? Adjust "Sidebar width" with `←`/`→` in ⚙ Settings. The column
  target persists and is re-applied when the surrounding tab width changes.
- Prefer your terminal's palette? Switch "Color theme" to `terminal`; ANSI accents then
  follow the terminal profile while `vscode` preserves the original fixed RGB colors.

### 🔀 Source Control

<div align="center">
<img src="plugins/herdr-sidebar/docs/media/source-control.png" alt="Source control: multi-repo staging, per-repo commit boxes, history drawers" width="920">
</div>

Everything you reach for in an editor's source-control panel, in a terminal pane:

- **Click a change, see the diff** — every changed file opens its colored `git diff` in
  a preview tab (staged vs working tree respected, untracked shown as additions), and
  the diff live-updates while you edit. Double-click to pin the tab; commits, stashes,
  branches and tags in the history drawers open the same way.
- **Stage, unstage, discard, commit** — by key or click, with Staged/Changes sections,
  count badges, and familiar per-file status letters.
- **✧ AI commit messages** — the sparkle button sends the pending diff to your local
  `claude` CLI and drops a drafted subject line into the message box. No claude? A clean
  filename-based fallback kicks in. Never blocks the UI.
- **Sync Changes** — a `⇅ 1↑ 2↓` button appears when you're ahead/behind upstream; one
  press runs `pull --rebase --autostash` + `push` in the background.
- **Multi-repo** — child repositories are auto-discovered, each with its
  own header (branch, dirty `*`, sync/commit icons), message box, and Commit button.
- **History drawers**: GRAPH, COMMITS, FILE HISTORY (follows your selection), BRANCHES,
  REMOTES, STASHES, TAGS.
- **Auto-refreshing** — commits and edits made anywhere show up within seconds.

## Prefer two panels? Take two panels.

<div align="center">
<img src="plugins/herdr-sidebar/docs/media/separated.png" alt="Separated Source Control and Explorer panes" width="920">
</div>

The ⚙ settings modal — mouse-toggleable like everything else — flips between:

- **Unified sidebar**: both views share one pane, the activity bar switches instantly.
- **Separated panels**: Explorer and Source Control as independent side-by-side panes —
  each keeping the full sidebar width.

<div align="center">
<img src="plugins/herdr-sidebar/docs/media/settings.png" alt="The settings modal" width="920">
</div>

Dock side, sidebar width, icon theme, dotfile visibility, live-folder following, and the full hotkey
reference live in the same modal (with a toggle if you'd rather keep the key hints
pinned to the sidebar's footer), and every choice persists across restarts. However you
split it, the dock takes care of itself: a focus hook re-docks the sidebar in any tab or
workspace that's missing one — new project, new worktree, new window, it's just *there*.

## Install

```
herdr plugin install alexarthurs/herdr-sidebar/plugins/herdr-sidebar
```

or from a local checkout:

```
cd plugins/herdr-sidebar
cargo build --release
herdr plugin link .
```

Open it with an action (or just focus a tab and let the hook dock it):

```
herdr plugin action invoke herdr-sidebar.open-sidebar-windows   # windows
herdr plugin action invoke herdr-sidebar.open-sidebar           # linux / macos
```

**Requirements:** Rust to build, herdr ≥ 0.8. **Recommended:** a Nerd Font terminal face
for the material icons — without one the sidebar auto-starts in its emoji theme, which
renders in any font. Note Windows Terminal's bundled Cascadia does NOT include the icon
glyphs; grab a patched font in one command and select it in your terminal profile:

```
winget install DEVCOM.JetBrainsMonoNerdFont
```

(or any font from [nerdfonts.com](https://www.nerdfonts.com/font-downloads), e.g.
CaskaydiaCove). Also recommended: the
[`claude` CLI](https://claude.com/claude-code) for ✧ commit messages.

## Keys

| Explorer | | Source Control | |
|---|---|---|---|
| `↑↓` / `jk` | move | `⏎` | stage / unstage |
| `←→` / `hl` | fold / unfold | `a` / `u` | stage all / none |
| `⏎` | toggle folder · preview file | `c` | focus message box |
| `r` | refresh | `A` | ✧ suggest message |
| `.` | dotfiles | `S` | sync ↑↓ |
| `m` | context menu | `o` | open diff |
| `b` | hide sidebar | `m` | context menu |
| `s` | settings | `b` | hide sidebar |
| `1` / `2` | switch view | `s` | settings |
| | | `1` / `2` | switch view |

In a preview tab: drag to select rendered text, `Ctrl/Cmd+C` copy, `↑↓` scroll,
`⇞⇟` page, `g`/`G` ends, `w` toggle line wrapping, `q` close the tab.

…and the mouse for all of it: click, double-click, scroll, hover, Ctrl+right-click menus.
`m` opens the same menus from the keyboard, for terminals and mobile clients that have
no right-click.

### Experimental editor

| Key | Action |
|---|---|
| `e` | enter edit mode from a regular file preview |
| arrows · `Home` / `End` · `PageUp` / `PageDown` | navigate logical and wrapped rows |
| `Shift` + navigation | select text |
| click / drag / Shift+click | move the caret / select / extend selection |
| `Ctrl/Cmd+A` · `Ctrl/Cmd+C/X/V` | select all · copy/cut/paste (clipboard where available) |
| `Ctrl/Cmd+F` | find; `Enter` next, `Esc` close find |
| `Ctrl/Cmd+S` | explicitly save |
| `Esc` | return to read-only preview; prompts if dirty |
| `Ctrl/Cmd+Q` | close the pane; prompts if dirty |

Edit mode accepts valid UTF-8 text files up to the preview limits (1 MiB / 5000 lines).
UTF-8 BOM and the file's LF/CRLF convention are preserved. Clean buffers reload external
changes automatically; dirty buffers show an external-change warning and require an explicit
overwrite or reload decision at save time.

## Actions

| Action | What it does |
|---|---|
| `open-sidebar` / `open-sidebar-windows` | Toggle the sidebar: open at its configured edge / focus / close |
| `open-git` / `open-git-windows` | Toggle a separate Source Control pane (separated mode) |
| `redeploy` / `redeploy-windows` | After a rebuild: refresh every workspace onto the new build |

Pressing `b` inside the plugin hides that tab's sidebar, snoozes auto-open there, and
remembers the **workspace** as hidden so a later click on another pane (Perforce, shell)
does not bring it back. Toggle it open again and that workspace is remembered as shown.

Herdr's built-in `prefix+b` (`Ctrl+b` then `b`) toggles **Herdr's own** workspace/agent
sidebar, not this plugin. Bind a different chord — do not steal `prefix+b` unless you
want that:

```toml
[[keys.command]]
key = "prefix+shift+b"
type = "shell"
command = "herdr plugin action invoke open-sidebar-windows --plugin herdr-sidebar" # Windows
description = "toggle herdr-sidebar"
```

Use `open-sidebar` instead of `open-sidebar-windows` on Linux or macOS, then run
`herdr server reload-config`. You can also run that invoke from any pane.

## Under the hood

- **One self-contained Rust crate** — ratatui + crossterm + serde, nothing else. Both
  views compile into one binary; separated panes are the same binary pinned with `--view`.
- All herdr control (docking, labels, identity tokens, pane spawning) goes over **herdr's
  socket API directly**; the Windows focus hooks run a windowless GUI-subsystem sidecar so
  nothing ever flashes a console window.
- Both dock sides survive real layouts — edge-aware split/swap, mirrored full-height repair,
  ratio-aware resizing, and preview-tab docking are unit-tested against herdr's actual JSON.
- Windows quirks (exe locking, PowerShell 5.1 BOMs, double-width Nerd Font glyphs) are
  handled, and the hard-won findings are documented in [`CLAUDE.md`](CLAUDE.md).

---

<div align="center">
<sub>Screenshots: herdr on Windows Terminal, CaskaydiaCove Nerd Font.</sub>
</div>
