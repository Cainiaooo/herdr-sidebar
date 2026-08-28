//! Unified-sidebar state: which layout the user chose (one combined Sidebar
//! pane vs separate Explorer / Source Control panes) and which view was
//! active last, persisted in a small JSON file so every pane and launcher
//! agrees across restarts.
//!
//! - `merged`: the unified sidebar is on (survives restarts).
//! - `active`: the view shown last, so a fresh sidebar opens where the user
//!   left off.
//! - `follow_cwd`: follow the live cwd of the neighbouring pane.
//! - `dock_right`: dock at the right edge instead of the default left edge.
//! - `sidebar_width`: preferred sidebar width in terminal columns.
//!
//! Both views live in ONE binary; switching is an in-process re-render, and
//! separated panes are the same binary pinned to a starting view with
//! `--view`.

use std::path::{Path, PathBuf};

/// Pane label (and metadata identity) of the unified pane.
pub const SIDEBAR_LABEL: &str = "Sidebar";

pub const DEFAULT_SIDEBAR_WIDTH: u16 = 32;
pub const MIN_SIDEBAR_WIDTH: u16 = 24;
pub const MAX_SIDEBAR_WIDTH: u16 = 80;
pub const SIDEBAR_WIDTH_STEP: u16 = 4;

pub fn clamp_sidebar_width(width: u16) -> u16 {
    width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
}

pub fn step_sidebar_width(width: u16, wider: bool) -> u16 {
    let width = clamp_sidebar_width(width);
    if wider {
        (width + SIDEBAR_WIDTH_STEP).min(MAX_SIDEBAR_WIDTH)
    } else {
        width.saturating_sub(SIDEBAR_WIDTH_STEP).max(MIN_SIDEBAR_WIDTH)
    }
}

/// Shell-agnostic command name typed into panes we create. [`spawn_env`]
/// prepends this binary's directory to PATH, so PowerShell, cmd, sh, bash,
/// nushell, and pwsh all resolve the same bare executable name.
pub const EXECUTABLE_NAME: &str = "herdr-sidebar";

/// The viewer's control path travels in the pane environment rather than in
/// a shell-quoted argv. Paths can contain spaces and every supported shell
/// has different quoting/call syntax.
pub const PREVIEW_CONTROL_ENV: &str = "HERDR_SIDEBAR_PREVIEW_CONTROL";

/// Unix seconds now — the heartbeat clock for pane identity tokens.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Why a view's event loop ended; main.rs acts on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    Quit,
    /// The user picked the other view — main re-renders in process.
    Switch,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Explorer,
    SourceControl,
}

impl View {
    pub fn other(self) -> View {
        match self {
            View::Explorer => View::SourceControl,
            View::SourceControl => View::Explorer,
        }
    }

    /// The standalone pane label for this view.
    pub fn label(self) -> &'static str {
        match self {
            View::Explorer => "Explorer",
            View::SourceControl => "Source Control",
        }
    }

    /// The plugin that renders this view.
    pub fn plugin_id(self) -> &'static str {
        match self {
            View::Explorer => "herdr-sidebar-explorer",
            View::SourceControl => "herdr-sidebar-git",
        }
    }

    /// The `--view` flag value that pins a separated pane to this view.
    pub fn view_flag(self) -> &'static str {
        match self {
            View::Explorer => "explorer",
            View::SourceControl => "git",
        }
    }

    pub fn from_view_flag(flag: &str) -> Option<View> {
        match flag {
            "explorer" => Some(View::Explorer),
            "git" => Some(View::SourceControl),
            _ => None,
        }
    }

    /// The metadata token value this view reports on its pane.
    pub fn token(self) -> &'static str {
        match self {
            View::Explorer => "explorer",
            View::SourceControl => "source-control",
        }
    }

    fn state_name(self) -> &'static str {
        match self {
            View::Explorer => "explorer",
            View::SourceControl => "source-control",
        }
    }

    fn from_state_name(name: &str) -> Option<View> {
        match name {
            "explorer" => Some(View::Explorer),
            "source-control" => Some(View::SourceControl),
            _ => None,
        }
    }
}

/// Accent palette for the sidebar. `VsCode` preserves the historical RGB
/// styling; `Terminal` uses ANSI colors so the terminal profile remaps them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorTheme {
    VsCode,
    Terminal,
}

impl ColorTheme {
    pub fn label(self) -> &'static str {
        match self {
            Self::VsCode => "vscode",
            Self::Terminal => "terminal",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::VsCode => Self::Terminal,
            Self::Terminal => Self::VsCode,
        }
    }

    fn from_state_name(name: &str) -> Option<Self> {
        match name {
            "vscode" => Some(Self::VsCode),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

/// The sticky sidebar setting, shared by both plugins.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct State {
    pub merged: bool,
    pub active: View,
    /// Show the hotkey chips at the bottom of the sidebar (they always
    /// live in the ⚙ Settings modal; the footer copy is opt-in).
    pub show_hotkeys: bool,
    /// The user's explicit icon-theme choice; `None` = auto (Nerd Font
    /// probe). Set the moment they toggle `i` or the Settings row, so a
    /// wrong auto-guess is corrected once and stays corrected.
    pub icons: Option<crate::icons::IconTheme>,
    /// Accent palette. Terminal mode uses ANSI colors that inherit the
    /// terminal profile; VS Code preserves the original fixed RGB palette.
    pub color_theme: ColorTheme,
    /// The first-run "install a Nerd Font?" prompt was answered (either
    /// way) — never show it again.
    pub font_prompt_done: bool,
    /// Default for workspaces with no hide/show record. Off = unknown spaces
    /// stay closed until the user toggles the sidebar open (issue #8); the
    /// explicit toggle always works. Hide/`b` and toggle write `visibility.json`
    /// so a later pane.focused in that space does not follow this default.
    pub auto_open: bool,
    /// The open-sidebar toggle treats an open-but-unfocused sidebar as CLOSE
    /// instead of FOCUS: one press opens, the next press closes, wherever
    /// focus is. Off keeps the historical open / focus / close cycle.
    pub strict_toggle: bool,
    /// Focus the sidebar once the toggle opens it. Off docks it in the
    /// background: focus stays in the pane the toggle was invoked from.
    pub focus_on_open: bool,
    /// Follow the live foreground cwd of a non-sidebar pane in this tab.
    /// Manual folder choices win until an already-observed pane changes cwd.
    pub follow_cwd: bool,
    /// Decorate the Explorer tree with git status (issue #19). Off stops the
    /// background `git status` polling entirely — the escape hatch for a repo
    /// where status is slow.
    pub git_deco: bool,
    /// Dock the sidebar at the right edge of each tab. False preserves the
    /// historical left dock.
    pub dock_right: bool,
    /// Preferred pane width in terminal columns. Layout code keeps this
    /// column target in the normal range and yields proportionally when the
    /// tab becomes unusually narrow.
    pub sidebar_width: u16,
}

impl Default for State {
    fn default() -> Self {
        Self {
            merged: true,
            active: View::Explorer,
            show_hotkeys: false,
            icons: None,
            color_theme: ColorTheme::VsCode,
            font_prompt_done: false,
            auto_open: true,
            strict_toggle: false,
            focus_on_open: true,
            follow_cwd: true,
            git_deco: true,
            dock_right: false,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        }
    }
}

pub fn follow_cwd_setting_value(enabled: bool) -> String {
    let value = if enabled { "on" } else { "off" };
    if cfg!(windows) {
        format!("{value} (host n/a)")
    } else {
        value.to_string()
    }
}

/// Durable state belongs in herdr's per-plugin state dir (docs: "store
/// runtime state in HERDR_PLUGIN_STATE_DIR"). herdr injects that env for
/// hooks/actions but NOT panes, so our launchers pass it into every pane
/// they split (see [`spawn_env`]); when it didn't reach us, fall back to
/// the conventional location herdr resolves it to.
pub fn state_path() -> Option<PathBuf> {
    Some(state_dir()?.join("state.json"))
}

fn state_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("HERDR_PLUGIN_STATE_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")));
    Some(base?.join("herdr").join("plugins").join("herdr-sidebar"))
}

/// Env for panes WE spawn. Panes don't inherit the hook/action env herdr
/// injects, so forward the state dir and prepend the directory containing
/// our executable to PATH. Launchers can then type the same bare command in
/// every configured shell without quoting an absolute path.
pub fn spawn_env() -> serde_json::Value {
    let mut env = serde_json::Map::new();
    if let Some(dir) = state_dir() {
        env.insert(
            "HERDR_PLUGIN_STATE_DIR".into(),
            serde_json::Value::String(dir.display().to_string()),
        );
    }
    if let Some(path) = launch_path() {
        env.insert("PATH".into(), serde_json::Value::String(path));
    }
    serde_json::Value::Object(env)
}

fn launch_path() -> Option<String> {
    let bin_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let mut paths = vec![bin_dir.clone()];
    paths.extend(
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .filter(|path| path != &bin_dir),
    );
    std::env::join_paths(paths)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// The pre-rename location (`%APPDATA%\herdr\aa-sidebar.json` / the XDG
/// config dir), read once so existing settings survive the migration.
fn legacy_state_path() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    Some(base?.join("herdr").join("aa-sidebar.json"))
}

pub fn load_state() -> State {
    if let Some(json) = state_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        return parse_state(&json);
    }
    // One-time migration from the legacy config-dir file.
    if let Some(json) = legacy_state_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        let state = parse_state(&json);
        save_state(state);
        return state;
    }
    State::default()
}

/// Best-effort persist; the sidebar still works for this session if it fails.
pub fn save_state(state: State) {
    let Some(path) = state_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Some(_lock) = StateWriteLock::acquire(&path) else {
        return;
    };
    write_state(&path, state);
}

/// Atomically-with-respect-to-other-sidebar-processes update one or more
/// settings. Every caller reloads after taking the lock, so a tab that has
/// been open for hours cannot overwrite newer fields from another tab.
pub fn update_state(update: impl FnOnce(&mut State)) -> State {
    let mut state = load_state();
    let Some(path) = state_path() else {
        update(&mut state);
        return state;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Some(_lock) = StateWriteLock::acquire(&path) else {
        update(&mut state);
        return state;
    };
    if let Ok(json) = std::fs::read_to_string(&path) {
        state = parse_state(&json);
    }
    update(&mut state);
    write_state(&path, state);
    state
}

fn write_state(path: &Path, state: State) {
    let icons = match state.icons {
        Some(theme) => format!(",\"icons\":\"{}\"", theme.state_name()),
        None => String::new(),
    };
    let json = format!(
        "{{\"merged\":{},\"active\":\"{}\",\"hotkeys\":{},\"font_prompt\":{},\"auto_open\":{},\"strict_toggle\":{},\"focus_on_open\":{},\"follow_cwd\":{},\"git_deco\":{},\"dock_right\":{},\"sidebar_width\":{},\"colors\":\"{}\"{icons}}}",
        state.merged,
        state.active.state_name(),
        state.show_hotkeys,
        state.font_prompt_done,
        state.auto_open,
        state.strict_toggle,
        state.focus_on_open,
        state.follow_cwd,
        state.git_deco,
        state.dock_right,
        clamp_sidebar_width(state.sidebar_width),
        state.color_theme.label()
    );
    let _ = std::fs::write(path, json);
}

struct StateWriteLock {
    path: PathBuf,
}

impl StateWriteLock {
    fn acquire(state_path: &Path) -> Option<Self> {
        let path = state_path.with_extension("lock");
        for _ in 0..50 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Some(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(5));
                    if stale {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for StateWriteLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The shape of the tree a freshly opened sidebar should start with, so a
/// new tab mirrors what the user was already looking at. Kept beside
/// `state.json` rather than in [`State`], which stays `Copy` because it is
/// passed by value everywhere.
///
/// Captured at sidebar startup only — expanding a folder in one tab does not
/// reach into tabs that are already open.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct TreeState {
    pub expanded: Vec<PathBuf>,
    pub selected: Option<PathBuf>,
}

fn tree_path() -> Option<PathBuf> {
    Some(state_dir()?.join("tree.json"))
}

/// The whole file: tree state per workspace ROOT. One file serves every
/// sidebar in the session, and each agent's tab is rooted somewhere
/// different, so a single unkeyed entry let one project's expansion and
/// selection load under another project's root.
type TreeFile = serde_json::Map<String, serde_json::Value>;

/// Forgiving decode: anything missing, truncated, or written before this file
/// was root-keyed yields an empty map rather than wedging the tree.
fn decode_tree_file(json: &str) -> TreeFile {
    serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| match v {
            // Pre-root-keyed shapes (a bare array, then a flat
            // {expanded, selected}) cannot be attributed to a root, so they
            // are dropped instead of applied to whoever opens first.
            serde_json::Value::Object(m) if !m.contains_key("expanded") => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// One root's entry.
fn tree_state_for(file: &TreeFile, root: &Path) -> TreeState {
    let paths = |v: Option<&serde_json::Value>| -> Vec<PathBuf> {
        v.and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let Some(entry) = file.get(&root.display().to_string()) else {
        return TreeState::default();
    };
    TreeState {
        expanded: paths(entry.get("expanded")),
        selected: entry
            .get("selected")
            .and_then(|v| v.as_str())
            .map(PathBuf::from),
    }
}

/// The tree state saved for `root`, for a sidebar starting up in it.
pub fn load_tree_state(root: &Path) -> TreeState {
    let Some(json) = tree_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return TreeState::default();
    };
    tree_state_for(&decode_tree_file(&json), root)
}

/// Best-effort persist of `root`'s entry, leaving every other root's alone.
/// Losing it only costs the next sidebar its starting shape.
pub fn save_tree_state(root: &Path, state: &TreeState) {
    let Some(path) = tree_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Some(_lock) = StateWriteLock::acquire(&path) else {
        return;
    };
    // Read-modify-write: concurrent sidebars in DIFFERENT roots must not
    // erase each other's entries. Two sidebars in the SAME root race, and
    // last-writer-wins is fine — they hold the same tree.
    let mut file = std::fs::read_to_string(&path)
        .map(|json| decode_tree_file(&json))
        .unwrap_or_default();
    file.insert(
        root.display().to_string(),
        serde_json::json!({
            "expanded": state.expanded.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "selected": state.selected.as_ref().map(|p| p.display().to_string()),
        }),
    );
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = std::fs::write(path, json);
    }
}

// ---------------------------------------------------------------------------
// The source-control view's shape, mirrored into new tabs (parallel to
// the explorer's tree state). Keyed by the sidebar's cwd: a preview tab's
// sidebar is spawned with the clicked repo as its cwd, so the two share a
// key when the originating sidebar already lived in that repo.
// ---------------------------------------------------------------------------

/// The SCM view state worth mirroring into a fresh sidebar: which drawers
/// are expanded (by title), the active repo's root, a stable id for the
/// selected row, the FILE HISTORY target, the scroll offset, and commit
/// drafts keyed by repository root.
#[derive(Default)]
pub struct ScmState {
    pub drawers: Vec<String>,
    pub active_root: Option<String>,
    pub selected: Option<String>,
    pub history_target: Option<String>,
    pub scroll: usize,
    pub drafts: std::collections::BTreeMap<String, String>,
    /// Draft roots this pane previously observed and has since emptied.
    /// Kept out of the JSON shape; it only scopes merge-on-write removals.
    pub cleared_drafts: std::collections::BTreeSet<String>,
}

fn scm_path() -> Option<PathBuf> {
    Some(state_dir()?.join("scm.json"))
}

/// Stable JSON key for a filesystem path. Git commonly reports `/` on
/// Windows while pane cwd values use `\`; treating those spellings as
/// different loses mirrored SCM state in preview tabs.
pub fn scm_path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Forgiving decode: anything missing, truncated, or shaped before this file
/// was cwd-keyed yields an empty map rather than wedging the view.
type ScmFile = serde_json::Map<String, serde_json::Value>;

fn decode_scm_file(json: &str) -> ScmFile {
    serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| match v {
            serde_json::Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// One cwd's entry, decoded into [`ScmState`]. Unknown/missing fields default.
fn scm_state_for(file: &ScmFile, cwd: &Path) -> ScmState {
    let key = scm_path_key(cwd);
    let Some(entry) = file.get(&key).or_else(|| {
        file.iter()
            .find(|(stored, _)| scm_path_key(Path::new(stored)) == key)
            .map(|(_, entry)| entry)
    }) else {
        return ScmState::default();
    };
    let drawers = entry
        .get("drawers")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    ScmState {
        drawers,
        active_root: entry
            .get("active_root")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        selected: entry
            .get("selected")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        history_target: entry
            .get("history_target")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        scroll: entry.get("scroll").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        drafts: entry
            .get("drafts")
            .and_then(|v| v.as_object())
            .map(|drafts| {
                drafts
                    .iter()
                    .filter_map(|(root, message)| {
                        message
                            .as_str()
                            .map(|message| (scm_path_key(Path::new(root)), message.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        cleared_drafts: std::collections::BTreeSet::new(),
    }
}

/// The SCM view saved for `cwd`, for a sidebar starting up in it.
pub fn load_scm_state(cwd: &Path) -> ScmState {
    let Some(json) = scm_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return ScmState::default();
    };
    scm_state_for(&decode_scm_file(&json), cwd)
}

/// Persist `cwd`'s entry, leaving every other cwd's alone. Returns false when
/// the write cannot be completed, so a graceful-close caller can keep the TUI
/// alive instead of acknowledging data it did not save.
pub fn save_scm_state(cwd: &Path, state: &ScmState) -> bool {
    let Some(path) = scm_path() else { return false };
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return false;
    }
    let Some(_lock) = StateWriteLock::acquire(&path) else {
        return false;
    };
    let mut file = std::fs::read_to_string(&path)
        .map(|json| decode_scm_file(&json))
        .unwrap_or_default();
    let key = scm_path_key(cwd);
    let mut drafts = scm_state_for(&file, cwd).drafts;
    merge_scm_drafts(&mut drafts, state);
    file.retain(|stored, _| scm_path_key(Path::new(stored)) != key);
    file.insert(
        key,
        serde_json::json!({
            "drawers": state.drawers,
            "active_root": state.active_root,
            "selected": state.selected,
            "history_target": state.history_target,
            "scroll": state.scroll,
            "drafts": drafts,
        }),
    );
    serde_json::to_string(&file)
        .ok()
        .is_some_and(|json| std::fs::write(path, json).is_ok())
}

fn merge_scm_drafts(
    stored: &mut std::collections::BTreeMap<String, String>,
    state: &ScmState,
) {
    for root in &state.cleared_drafts {
        stored.remove(root);
    }
    stored.extend(state.drafts.clone());
}

// ---------------------------------------------------------------------------
// The root each space's tree is built from.
// ---------------------------------------------------------------------------

/// Remembered tree roots, keyed by workspace LABEL.
///
/// Not by workspace id: ids identify a space *instance*, not a project —
/// closing and recreating `tremor` moved it from `wG` to `wH` within one
/// session — so an id key would hand a new space a root chosen for an
/// unrelated one. The label is intrinsic to the project and survives a server
/// restart; renaming a space forgets its root, which is the accepted cost.
type RootsFile = serde_json::Map<String, serde_json::Value>;

fn roots_path() -> Option<PathBuf> {
    Some(state_dir()?.join("roots.json"))
}

/// Forgiving decode: anything missing or garbled yields an empty map, so a
/// hand-edited file forgets a choice rather than wedging the tree.
fn decode_roots_file(json: &str) -> RootsFile {
    serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| match v {
            serde_json::Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// The root remembered for `label`, if any. An empty label never matches —
/// it would collide across every space that failed to report one.
fn root_for_label(file: &RootsFile, label: &str) -> Option<PathBuf> {
    if label.is_empty() {
        return None;
    }
    file.get(label).and_then(|v| v.as_str()).map(PathBuf::from)
}

/// The root this space's tree should use, or `None` to fall back to the
/// pane's cwd.
pub fn load_root(label: &str) -> Option<PathBuf> {
    let json = roots_path().and_then(|p| std::fs::read_to_string(p).ok())?;
    root_for_label(&decode_roots_file(&json), label)
}

/// Remember `root` for `label`, leaving other spaces' choices alone.
pub fn save_root(label: &str, root: &Path) {
    if label.is_empty() {
        return;
    }
    let Some(path) = roots_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Some(_lock) = StateWriteLock::acquire(&path) else {
        return;
    };
    let mut file = std::fs::read_to_string(&path)
        .map(|json| decode_roots_file(&json))
        .unwrap_or_default();
    file.insert(
        label.to_string(),
        serde_json::json!(root.display().to_string()),
    );
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = std::fs::write(path, json);
    }
}

/// Per-workspace "should this space auto-dock a sidebar?" remembered from
/// an explicit toggle or hide. Keyed like [`save_root`]: by workspace
/// **label**, falling back to the workspace id when the label is unknown.
///
/// Unknown spaces follow the global `auto_open` setting. Hide/`b` records
/// off; the open-sidebar toggle records on. Quiet ensure hooks must not
/// write this file — auto-docking is not a user choice.
type VisibilityFile = serde_json::Map<String, serde_json::Value>;

fn visibility_path() -> Option<PathBuf> {
    Some(state_dir()?.join("visibility.json"))
}

fn decode_visibility_file(json: &str) -> VisibilityFile {
    serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|v| match v {
            serde_json::Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

fn visibility_key(label: &str, workspace_id: &str) -> String {
    if !label.is_empty() {
        label.to_string()
    } else {
        workspace_id.to_string()
    }
}

fn recorded_visible(file: &VisibilityFile, key: &str) -> Option<bool> {
    if key.is_empty() {
        return None;
    }
    file.get(key).and_then(|v| v.as_bool())
}

fn auto_open_decision(global: bool, recorded: Option<bool>) -> bool {
    recorded.unwrap_or(global)
}

pub fn should_auto_open_workspace(global: bool, label: &str, workspace_id: &str) -> bool {
    let key = visibility_key(label, workspace_id);
    let recorded = visibility_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|json| decode_visibility_file(&json))
        .and_then(|file| recorded_visible(&file, &key));
    auto_open_decision(global, recorded)
}

/// Persist a user choice for this space. Empty label+id is ignored so we
/// never write a catch-all key that would clobber every unnamed workspace.
pub fn remember_visible(label: &str, workspace_id: &str, visible: bool) {
    let key = visibility_key(label, workspace_id);
    if key.is_empty() {
        return;
    }
    let Some(path) = visibility_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Some(_lock) = StateWriteLock::acquire(&path) else {
        return;
    };
    let mut file = std::fs::read_to_string(&path)
        .map(|json| decode_visibility_file(&json))
        .unwrap_or_default();
    file.insert(key, serde_json::json!(visible));
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = std::fs::write(path, json);
    }
}

pub fn remember_visible_for_pane(
    pane_list_json: &str,
    workspace_list_json: &str,
    pane_id: &str,
    visible: bool,
) {
    let workspace_id = crate::launch::workspace_of(pane_list_json, pane_id);
    let label = crate::launch::workspace_label(workspace_list_json, &workspace_id);
    remember_visible(&label, &workspace_id, visible);
}

/// Forgiving parse: any missing/garbled field falls back to the default, so a
/// hand-edited or truncated file can never wedge the panels.
pub fn parse_state(json: &str) -> State {
    let value: serde_json::Value = match serde_json::from_str(json.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(_) => return State::default(),
    };
    let default = State::default();
    State {
        merged: value
            .get("merged")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.merged),
        active: value
            .get("active")
            .and_then(|v| v.as_str())
            .and_then(View::from_state_name)
            .unwrap_or(default.active),
        show_hotkeys: value
            .get("hotkeys")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.show_hotkeys),
        icons: value
            .get("icons")
            .and_then(|v| v.as_str())
            .and_then(crate::icons::IconTheme::from_state_name),
        color_theme: value
            .get("colors")
            .and_then(|v| v.as_str())
            .and_then(ColorTheme::from_state_name)
            .unwrap_or(default.color_theme),
        font_prompt_done: value
            .get("font_prompt")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.font_prompt_done),
        auto_open: value
            .get("auto_open")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.auto_open),
        strict_toggle: value
            .get("strict_toggle")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.strict_toggle),
        focus_on_open: value
            .get("focus_on_open")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.focus_on_open),
        follow_cwd: value
            .get("follow_cwd")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.follow_cwd),
        git_deco: value
            .get("git_deco")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.git_deco),
        dock_right: value
            .get("dock_right")
            .and_then(|v| v.as_bool())
            .unwrap_or(default.dock_right),
        sidebar_width: value
            .get("sidebar_width")
            .and_then(|v| v.as_u64())
            .and_then(|v| u16::try_from(v).ok())
            .map(clamp_sidebar_width)
            .unwrap_or(default.sidebar_width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Workspace IDs are per-space-INSTANCE, not per-project: closing and
    /// recreating `tremor` moved it from `wG` to `wH` inside one session.
    /// Keying a remembered root on the id would hand a future space the root
    /// picked for an unrelated one, so the label is the key.
    #[test]
    fn remembered_roots_are_keyed_by_workspace_label() {
        let file = decode_roots_file(r#"{"tremor":"/repo/tremor","faultline":"/repo/faultline"}"#);
        assert_eq!(
            root_for_label(&file, "tremor"),
            Some(PathBuf::from("/repo/tremor"))
        );
        assert_eq!(
            root_for_label(&file, "faultline"),
            Some(PathBuf::from("/repo/faultline"))
        );
        // An unknown space has made no choice yet — the caller falls back to cwd.
        assert_eq!(root_for_label(&file, "bedrock"), None);
        // An empty label must never match; it would collide across spaces.
        assert_eq!(root_for_label(&file, ""), None);
    }

    #[test]
    fn a_garbled_roots_file_forgets_rather_than_wedges() {
        for junk in ["garbage", "[]", r#"{"tremor":42}"#, ""] {
            assert_eq!(
                root_for_label(&decode_roots_file(junk), "tremor"),
                None,
                "{junk}"
            );
        }
    }

    /// One state file serves every sidebar in the session, so it has to be
    /// keyed by tree ROOT. Keyed globally, a sidebar spawned in another
    /// agent's tab loaded whatever project the user last touched — their
    /// expansion and selection appeared under a different agent's root.
    #[test]
    fn tree_state_is_per_root_so_agents_do_not_bleed_into_each_other() {
        let a = PathBuf::from("/repo/faultline");
        let b = PathBuf::from("/repo/tremor");
        let file = decode_tree_file(
            r#"{"/repo/faultline":{"expanded":["/repo/faultline/src"],
                                   "selected":"/repo/faultline/src/main.rs"},
                "/repo/tremor":{"expanded":["/repo/tremor/lib"],"selected":null}}"#,
        );
        assert_eq!(tree_state_for(&file, &a).expanded, vec![a.join("src")]);
        assert_eq!(
            tree_state_for(&file, &a).selected,
            Some(a.join("src/main.rs"))
        );
        assert_eq!(tree_state_for(&file, &b).expanded, vec![b.join("lib")]);
        assert_eq!(
            tree_state_for(&file, &b).selected,
            None,
            "tremor has no selection"
        );
        // An unknown root starts fresh instead of inheriting someone else's.
        assert_eq!(
            tree_state_for(&file, Path::new("/repo/other")),
            TreeState::default()
        );
    }

    #[test]
    fn scm_keys_ignore_windows_separator_spelling() {
        let file = decode_scm_file(
            r#"{"C:/repo":{"drawers":["CHANGES"],"active_root":"C:/repo","scroll":4,"drafts":{"C:/repo":"keep me"}}}"#,
        );
        let state = scm_state_for(&file, Path::new(r"C:\repo"));
        assert_eq!(state.drawers, vec!["CHANGES"]);
        assert_eq!(state.active_root.as_deref(), Some("C:/repo"));
        assert_eq!(state.scroll, 4);
        assert_eq!(state.drafts.get("C:/repo").map(String::as_str), Some("keep me"));
        assert_eq!(scm_path_key(Path::new(r"C:\repo\src")), "C:/repo/src");
    }

    #[test]
    fn scm_draft_merge_preserves_unobserved_sibling_writes() {
        let mut stored = std::collections::BTreeMap::from([
            ("/repo/a".to_string(), "newer sibling draft".to_string()),
            ("/repo/b".to_string(), "draft to clear".to_string()),
        ]);
        let mut state = ScmState::default();
        state
            .cleared_drafts
            .insert("/repo/b".to_string());

        merge_scm_drafts(&mut stored, &state);

        assert_eq!(
            stored.get("/repo/a").map(String::as_str),
            Some("newer sibling draft")
        );
        assert!(!stored.contains_key("/repo/b"));
    }

    /// Older files were a bare array, then a flat object. Both predate
    /// root-keying and cannot be attributed to a root, so they are dropped
    /// rather than applied to whichever project opens first.
    #[test]
    fn pre_root_keyed_tree_files_are_discarded_not_misapplied() {
        for legacy in [
            r#"["/r/src"]"#,
            r#"{"expanded":["/r/src"],"selected":"/r/src/main.rs"}"#,
            "garbage",
        ] {
            let file = decode_tree_file(legacy);
            assert_eq!(
                tree_state_for(&file, Path::new("/r")),
                TreeState::default(),
                "{legacy}"
            );
        }
    }

    #[test]
    fn state_roundtrip_and_forgiving_parse() {
        let state = State {
            merged: true,
            active: View::SourceControl,
            show_hotkeys: true,
            icons: Some(crate::icons::IconTheme::Emoji),
            color_theme: ColorTheme::Terminal,
            font_prompt_done: true,
            auto_open: false,
            strict_toggle: true,
            focus_on_open: false,
            follow_cwd: false,
            git_deco: false,
            dock_right: true,
            sidebar_width: 44,
        };
        let json = "{\"merged\":true,\"active\":\"source-control\",\"hotkeys\":true,\"font_prompt\":true,\"auto_open\":false,\"strict_toggle\":true,\"focus_on_open\":false,\"follow_cwd\":false,\"git_deco\":false,\"dock_right\":true,\"sidebar_width\":44,\"colors\":\"terminal\",\"icons\":\"emoji\"}";
        assert_eq!(parse_state(json), state);
        assert!(parse_state("\u{feff}{\"merged\":true}").merged);
        // Files written before the flag existed keep auto-open AND the git
        // decorations on.
        assert!(parse_state("{\"merged\":true}").auto_open);
        // Files written before the toggle settings existed keep the
        // historical toggle behavior: focus an open sidebar, focus on open.
        assert!(!parse_state("{\"merged\":true}").strict_toggle);
        assert!(parse_state("{\"merged\":true}").focus_on_open);
        assert_eq!(
            parse_state("{\"merged\":true}").color_theme,
            ColorTheme::VsCode
        );
        // Existing installs get neighbour following by default.
        assert!(parse_state("{\"merged\":true}").follow_cwd);
        assert!(parse_state("{\"merged\":true}").git_deco);
        // Files written before the dock setting existed stay left-docked.
        assert!(!parse_state("{\"merged\":true}").dock_right);
        assert_eq!(parse_state("{\"merged\":true}").sidebar_width, 32);
        assert_eq!(parse_state("{\"sidebar_width\":1}").sidebar_width, 24);
        assert_eq!(parse_state("{\"sidebar_width\":999}").sidebar_width, 80);
        assert_eq!(parse_state("garbage"), State::default());
        assert_eq!(parse_state("{\"active\":\"bogus\"}"), State::default());
    }

    #[test]
    fn sidebar_width_steps_and_saturates_within_supported_bounds() {
        assert_eq!(step_sidebar_width(32, true), 36);
        assert_eq!(step_sidebar_width(32, false), 28);
        assert_eq!(step_sidebar_width(MAX_SIDEBAR_WIDTH, true), MAX_SIDEBAR_WIDTH);
        assert_eq!(step_sidebar_width(MIN_SIDEBAR_WIDTH, false), MIN_SIDEBAR_WIDTH);
        assert_eq!(step_sidebar_width(1, true), 28);
        assert_eq!(step_sidebar_width(u16::MAX, false), 76);
    }

    #[test]
    fn follow_cwd_status_stays_compact() {
        if cfg!(windows) {
            assert_eq!(follow_cwd_setting_value(true), "on (host n/a)");
            assert_eq!(follow_cwd_setting_value(false), "off (host n/a)");
        } else {
            assert_eq!(follow_cwd_setting_value(true), "on");
            assert_eq!(follow_cwd_setting_value(false), "off");
        }
    }

    #[test]
    fn workspace_visibility_prefers_label_and_unknown_spaces_follow_auto_open() {
        assert_eq!(visibility_key("NeonGame", "w1"), "NeonGame");
        assert_eq!(visibility_key("", "w1"), "w1");
        assert_eq!(visibility_key("", ""), "");

        let file = decode_visibility_file(r#"{"NeonGame":false,"acme-app":true}"#);
        assert_eq!(recorded_visible(&file, "NeonGame"), Some(false));
        assert_eq!(recorded_visible(&file, "acme-app"), Some(true));
        assert_eq!(recorded_visible(&file, "other"), None);
        assert_eq!(recorded_visible(&file, ""), None);
        assert!(decode_visibility_file("garbage").is_empty());
        assert!(decode_visibility_file("[]").is_empty());

        assert!(!auto_open_decision(true, Some(false)));
        assert!(auto_open_decision(false, Some(true)));
        assert!(auto_open_decision(true, None));
        assert!(!auto_open_decision(false, None));
    }

    #[test]
    fn views_pair_up() {
        assert_eq!(View::Explorer.other(), View::SourceControl);
        assert_eq!(View::SourceControl.other(), View::Explorer);
        assert_eq!(View::Explorer.label(), "Explorer");
        assert_eq!(View::SourceControl.plugin_id(), "herdr-sidebar-git");
    }

    #[test]
    fn spawn_env_prepends_the_binary_directory_to_path() {
        let env = spawn_env();
        let path = env.get("PATH").and_then(|v| v.as_str()).unwrap();
        let first = std::env::split_paths(std::ffi::OsStr::new(path))
            .next()
            .unwrap();
        assert_eq!(first, std::env::current_exe().unwrap().parent().unwrap());
    }
}
