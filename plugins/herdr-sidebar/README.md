# herdr-sidebar

**Fork of this plugin** — see [`docs/FORK.md`](../../docs/FORK.md) and
[`docs/CHANGELOG.md`](../../docs/CHANGELOG.md). Link **this** checkout; do not
reinstall `alexarthurs/herdr-sidebar`.

**The sidebar your terminal was missing** — a VS Code-inspired file explorer + source
control panel in one dockable herdr pane.

<img src="docs/media/hero.png" alt="The sidebar Explorer and live preview interface" width="860">

**The full tour lives in the [repo README](../../README.md)** — features, screenshots,
keys, and settings.

## Install

Requires herdr 0.8 or newer.

```
herdr plugin install Cainiaooo/herdr-sidebar/plugins/herdr-sidebar --yes
```

(`--yes` when stdin is not a TTY.) That is this fork, not the public
`alexarthurs/herdr-sidebar` install.

or from a local checkout:

```
cargo build --release
herdr plugin link .
```

Open it (or just focus a tab — the hook docks it). Herdr's `Ctrl+b b` is **not**
this plugin; it toggles Herdr's own sidebar. Use the action, or bind e.g.
`prefix+shift+b` in `config.toml` (see the repo README):

```
herdr plugin action invoke herdr-sidebar.open-sidebar-windows   # windows
herdr plugin action invoke herdr-sidebar.open-sidebar           # linux / macos
```
