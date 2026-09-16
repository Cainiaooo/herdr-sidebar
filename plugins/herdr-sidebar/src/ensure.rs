//! Sidebar ensure/toggle, driven entirely over the socket API (see `ipc`) so a
//! focus-event hook never spawns a console process. Unix actions/hooks call
//! this through the main binary; Windows uses the GUI-subsystem sidecar. The
//! decision/plan parsing is the unit-tested `launch` module, fed the socket
//! responses (same JSON the CLI prints).

use std::fs::File;

use crate::{ipc, launch, state::View};

/// Why the native launcher was invoked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A focus/create hook quietly ensures the Explorer exists.
    Ensure,
    /// After a Herdr server restart: replace every restored corpse and
    /// re-dock tabs that should auto-open, without stealing focus.
    Restore,
    /// An explicit user action toggles the requested view.
    Toggle(View),
    /// An explicit host keybinding opens or focuses a specific activity.
    Activate(Target),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Explorer,
    Search,
    SourceControl,
    QuickOpen,
}

impl Target {
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value {
            "explorer" => Some(Self::Explorer),
            "search" => Some(Self::Search),
            "source-control" => Some(Self::SourceControl),
            "quick-open" => Some(Self::QuickOpen),
            _ => None,
        }
    }

    pub fn env_value(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::Search => "search",
            Self::SourceControl => "source-control",
            Self::QuickOpen => "quick-open",
        }
    }

    pub fn pane_view(self, merged: bool) -> View {
        match self {
            Self::SourceControl if !merged => View::SourceControl,
            _ => View::Explorer,
        }
    }

    pub fn initial_view(self) -> View {
        match self {
            Self::SourceControl => View::SourceControl,
            _ => View::Explorer,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Explorer => "f9",
            Self::Search => "f10",
            Self::SourceControl => "f11",
            Self::QuickOpen => "f12",
        }
    }
}

/// Serialize concurrent runs (pane/tab events arrive in bursts; unguarded,
/// one switch opened four panes). The OS releases this lock if a launcher
/// crashes, so no retry timer or stale-lock cleanup is needed.
pub struct LaunchLock {
    _file: File,
}

impl LaunchLock {
    /// Acquire the shared launcher lock. Discrete user actions should wait;
    /// redundant focus hooks should use a non-blocking attempt and yield.
    pub fn acquire(wait: bool) -> Option<Self> {
        let path = crate::state::state_path()
            .map(|path| path.with_file_name("launcher.lock"))
            .unwrap_or_else(|| std::env::temp_dir().join("herdr-sidebar-launcher.lock"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .ok()?;
        let acquired = if wait {
            file.lock().is_ok()
        } else {
            file.try_lock().is_ok()
        };
        acquired.then_some(Self { _file: file })
    }
}

use crate::snooze;

/// Quiet mode (hooks): make sure the focused tab has an Explorer, never moving
/// focus, and respecting a tab the user toggled closed. Toggle mode (the
/// action): open-or-focus-or-close, like VS Code's explorer shortcut.
pub fn run(mode: Mode) -> std::io::Result<()> {
    if matches!(mode, Mode::Restore) {
        return restore_all();
    }
    let toggle = matches!(mode, Mode::Toggle(_));
    let activation = match mode {
        Mode::Activate(target) => Some(target),
        _ => None,
    };
    let explicit = toggle || activation.is_some();
    let state = crate::state::load_state();
    let view = match mode {
        Mode::Ensure | Mode::Restore => View::Explorer,
        Mode::Toggle(view) => view,
        Mode::Activate(target) => target.pane_view(state.merged),
    };
    let event_json = std::env::var("HERDR_PLUGIN_EVENT_JSON").unwrap_or_default();
    let wait_for_lock = must_wait_for_lock(explicit, &event_json);
    let Some(_lock) = LaunchLock::acquire(wait_for_lock) else {
        return Ok(());
    };
    let mut panes = ipc::call_text("pane.list", serde_json::json!({}))?;
    let scope = if explicit {
        String::new()
    } else {
        let context_tab = std::env::var("HERDR_TAB_ID").unwrap_or_default();
        launch::event_scope_with_tab_context(&event_json, &panes, &context_tab)
    };
    let auto = toggle || workspace_should_auto_open(&state, &panes, &scope);
    let tab = launch::snooze_tab(&event_json, &panes, &scope);
    let snooze_dir = snooze::dir();
    snooze::sweep(&snooze_dir, &launch::live_tabs(&panes));
    let now = crate::state::unix_now();
    let decision_view = activation.map_or(view, |target| match target {
        Target::SourceControl => View::SourceControl,
        _ => View::Explorer,
    });
    let decision = match decision_view {
        View::Explorer => launch::launch_decision_in(&panes, now, &scope),
        View::SourceControl => launch::launch_decision_git(&panes, now),
    };
    let decision = if toggle && state.strict_toggle {
        launch::focus_as_close(&decision)
    } else {
        decision
    };
    let tracks_snooze = view == View::Explorer;
    match decision.split_once(' ') {
        Some(("FOCUS", id)) => {
            if toggle {
                if state.strict_toggle {
                    persist_hide_after_close(id, &snooze_dir, &tab, &panes, &scope);
                } else {
                    focus(id)?;
                    remember_scope(&panes, &scope, true);
                }
            } else if let Some(target) = activation {
                activate_existing(id, target)?;
            }
        }
        Some(("CLOSE", id)) => {
            if toggle {
                persist_hide_after_close(id, &snooze_dir, &tab, &panes, &scope);
            } else if let Some(target) = activation {
                activate_existing(id, target)?;
            }
        }
        Some(("REPLACE", id)) => {
            ipc::call_text("pane.close", serde_json::json!({ "pane_id": id }))?;
            if !auto {
                return Ok(());
            }
            panes = ipc::call_text("pane.list", serde_json::json!({}))?;
            if let Some(target) = activation {
                prepare_activation(target);
                open(&panes, true, &scope, view, Some(target), false)?;
            } else {
                open(
                    &panes,
                    toggle && state.focus_on_open,
                    &scope,
                    view,
                    None,
                    false,
                )?;
                if toggle {
                    remember_scope(&panes, &scope, true);
                }
            }
        }
        _ => {
            if toggle {
                if tracks_snooze {
                    snooze::clear(&snooze_dir, &tab);
                }
                open(&panes, state.focus_on_open, &scope, view, None, false)?;
                remember_scope(&panes, &scope, true);
            } else if let Some(target) = activation {
                if tracks_snooze {
                    snooze::clear(&snooze_dir, &tab);
                }
                prepare_activation(target);
                open(&panes, true, &scope, view, Some(target), false)?;
            } else if auto && !snooze::is_set(&snooze_dir, &tab) {
                open(&panes, false, &scope, view, None, false)?;
            }
        }
    }
    Ok(())
}

/// Herdr 0.9 restores pane labels after a server restart but does not replay
/// focus events for the already-focused workspace, and `launch_decision_in`
/// only sees the first explorer in a tab. Walk every corpse, close it, then
/// re-dock tabs that should auto-open. Hold the launcher lock the whole time
/// so the splits cannot re-enter ensure.
fn restore_all() -> std::io::Result<()> {
    let Some(_lock) = LaunchLock::acquire(true) else {
        return Ok(());
    };
    let mut panes = ipc::call_text("pane.list", serde_json::json!({}))?;
    let original_focus = launch::server_focused_pane_id(&panes);
    wait_for_session_layout(&original_focus);
    panes = ipc::call_text("pane.list", serde_json::json!({}))?;
    let state = crate::state::load_state();
    let snooze_dir = snooze::dir();
    snooze::sweep(&snooze_dir, &launch::live_tabs(&panes));
    let now = crate::state::unix_now();
    let corpses = launch::explorer_corpses(&panes, now);
    if corpses.is_empty() {
        return Ok(());
    }
    let mut tabs: Vec<String> = Vec::new();
    for (tab, pane_id) in &corpses {
        let _ = ipc::call_text("pane.close", serde_json::json!({ "pane_id": pane_id }));
        if !tabs.iter().any(|existing| existing == tab) {
            tabs.push(tab.clone());
        }
    }
    panes = ipc::call_text("pane.list", serde_json::json!({}))?;
    for tab in tabs {
        if snooze::is_set(&snooze_dir, &tab) {
            continue;
        }
        if !workspace_should_auto_open(&state, &panes, &tab) {
            continue;
        }
        let decision = launch::launch_decision_in(&panes, crate::state::unix_now(), &tab);
        if decision != "OPEN" {
            continue;
        }
        open(&panes, false, &tab, View::Explorer, None, true)?;
        panes = ipc::call_text("pane.list", serde_json::json!({}))?;
    }
    panes = ipc::call_text("pane.list", serde_json::json!({}))?;
    if !original_focus.is_empty() && launch::tab_of(&panes, &original_focus).is_empty() {
        return Ok(());
    }
    if !original_focus.is_empty() {
        let _ = focus(&original_focus);
    }
    Ok(())
}

fn wait_for_session_layout(pane_id: &str) {
    if pane_id.is_empty() {
        return;
    }
    for _ in 0..50 {
        if let Ok(layout) = ipc::call_text("pane.layout", serde_json::json!({ "pane_id": pane_id }))
            && launch::layout_width(&layout).unwrap_or(0) >= 20
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn workspace_should_auto_open(state: &crate::state::State, panes: &str, scope: &str) -> bool {
    let workspace_id = launch::workspace_id_from_scope(scope, panes);
    let label = ipc::call_text("workspace.list", serde_json::json!({}))
        .map(|json| launch::workspace_label(&json, &workspace_id))
        .unwrap_or_default();
    crate::state::should_auto_open_workspace(state.auto_open, &label, &workspace_id)
}

fn remember_scope(panes: &str, scope: &str, visible: bool) {
    let workspace_id = launch::workspace_id_from_scope(scope, panes);
    let label = ipc::call_text("workspace.list", serde_json::json!({}))
        .map(|json| launch::workspace_label(&json, &workspace_id))
        .unwrap_or_default();
    crate::state::remember_visible(&label, &workspace_id, visible);
}

fn persist_hide_after_close(
    pane_id: &str,
    snooze_dir: &std::path::Path,
    tab: &str,
    panes: &str,
    scope: &str,
) -> bool {
    if !graceful_close(pane_id) {
        return false;
    }
    snooze::set(snooze_dir, tab);
    remember_scope(panes, scope, false);
    true
}

fn graceful_close(pane_id: &str) -> bool {
    let _ = ipc::call_text(
        "pane.send_input",
        serde_json::json!({ "pane_id": pane_id, "text": "", "keys": ["ctrl+q"] }),
    );
    let mut acknowledged = false;
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Ok(json) = ipc::call_text("pane.list", serde_json::json!({}))
            && !launch::pane_has_token(&json, pane_id)
        {
            acknowledged = true;
            break;
        }
    }
    if acknowledged {
        let _ = ipc::call_text("pane.close", serde_json::json!({ "pane_id": pane_id }));
        true
    } else {
        let _ = ipc::call_text(
            "notification.show",
            serde_json::json!({
                "title": "Sidebar close cancelled",
                "body": "The pane did not acknowledge a safe shutdown; try again shortly.",
                "position": "bottom-right",
                "sound": "none",
            }),
        );
        false
    }
}

pub fn request_close(panes_json: &str, pane_id: &str) -> std::io::Result<()> {
    if launch::pane_is_starting(panes_json, pane_id) {
        ipc::call_text("pane.close", serde_json::json!({ "pane_id": pane_id }))?;
    } else {
        ipc::call_text(
            "pane.send_input",
            serde_json::json!({ "pane_id": pane_id, "text": "", "keys": ["ctrl+q"] }),
        )?;
    }
    Ok(())
}

fn focus(pane_id: &str) -> std::io::Result<()> {
    // The API has focus-by-id (`pane.focus`), unlike the CLI's zoom-cycle hack.
    ipc::call_text("pane.focus", serde_json::json!({ "pane_id": pane_id }))?;
    Ok(())
}

fn activate_existing(pane_id: &str, target: Target) -> std::io::Result<()> {
    prepare_activation(target);
    focus(pane_id)?;
    ipc::call_text(
        "pane.send_input",
        serde_json::json!({ "pane_id": pane_id, "text": "", "keys": [target.key()] }),
    )?;
    Ok(())
}

fn prepare_activation(target: Target) {
    crate::state::update_state(|state| match target {
        Target::Explorer | Target::QuickOpen => {
            state.active = View::Explorer;
            state.search_active = false;
        }
        Target::Search => {
            state.active = View::Explorer;
            state.search_active = true;
        }
        Target::SourceControl => {
            state.active = View::SourceControl;
            state.search_active = false;
        }
    });
}

fn open(
    panes_json: &str,
    focus_new: bool,
    scope: &str,
    view: View,
    initial: Option<Target>,
    skip_focus: bool,
) -> std::io::Result<()> {
    // Root the new sidebar from a pane in the scope we are docking into —
    // the decision above answered for that scope, and the two must agree or
    // we dock into one tab with another tab's cwd.
    let fp = launch::focused_pane_in(panes_json, scope);
    let Some((fid, fcwd)) = fp.split_once('\t') else {
        return Ok(());
    };

    let state = crate::state::load_state();
    let dock_right = state.dock_right;
    let layout = ipc::call_text("pane.layout", serde_json::json!({ "pane_id": fid }))?;
    let plan = launch::open_plan(&layout, dock_right, state.sidebar_width);
    let mut fields = plan.split('\t');
    let target = fields
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(fid)
        .to_string();
    let ratio = fields
        .next()
        .and_then(|r| r.parse::<f64>().ok())
        .unwrap_or(if dock_right { 0.75 } else { 0.25 });
    let needs_swap = fields
        .next()
        .and_then(|s| s.parse::<bool>().ok())
        .unwrap_or(!dock_right);

    #[cfg(unix)]
    let new_pane = ipc::open_plugin_pane(
        &target,
        view,
        std::path::Path::new(fcwd),
        view == View::Explorer && state.merged,
        initial.map(Target::env_value),
    )?;
    #[cfg(windows)]
    let new_pane = {
        let mut split = serde_json::json!({
            "target_pane_id": target,
            "direction": "right",
            "ratio": ratio,
            "focus": false,
        });
        if !fcwd.is_empty() {
            split["cwd"] = serde_json::Value::String(fcwd.to_string());
        }
        let mut env = crate::state::spawn_env();
        if let Some(initial) = initial
            && let Some(env) = env.as_object_mut()
        {
            env.insert(
                crate::state::INITIAL_ACTIVITY_ENV.to_string(),
                serde_json::Value::String(initial.env_value().to_string()),
            );
        }
        split["env"] = env;
        let response = ipc::call_text("pane.split", split)?;
        let Some(new_pane) = launch::split_pane_id(&response) else {
            return Ok(());
        };
        if let Err(error) =
            ipc::report_starting_identity(&new_pane, view, view == View::Explorer && state.merged)
        {
            let _ = ipc::call_text("pane.close", serde_json::json!({ "pane_id": new_pane }));
            return Err(error);
        }
        new_pane
    };

    if needs_swap {
        ipc::call_text(
            "pane.swap",
            serde_json::json!({ "source_pane_id": new_pane, "target_pane_id": target }),
        )?;
    }
    #[cfg(unix)]
    resize_direct_spawn(&new_pane, dock_right, ratio);
    #[cfg(windows)]
    {
        let command = if view == View::Explorer {
            crate::state::EXECUTABLE_NAME.to_string()
        } else {
            format!(
                "{} --view {}",
                crate::state::EXECUTABLE_NAME,
                view.view_flag()
            )
        };
        ipc::call_text(
            "pane.send_input",
            serde_json::json!({
                "pane_id": new_pane,
                "text": command,
                "keys": ["Enter"]
            }),
        )?;
        ipc::call_text(
            "pane.rename",
            serde_json::json!({ "pane_id": new_pane, "label": view.label() }),
        )?;
    }
    full_height_repair(&new_pane, dock_right);

    // Startup restore walks every workspace; focusing the split source
    // would yank the user out of the tab they are already in.
    if !skip_focus {
        if focus_new {
            focus(&new_pane)?;
        } else {
            // Quiet mode must never move focus, but the split/swap can (focus
            // follows the SLOT, not the pane) — unconditionally restore the pane
            // that was focused when we started.
            focus(fid)?;
        }
    }
    Ok(())
}

/// `plugin.pane.open` starts a direct process but currently exposes no split
/// ratio. It creates a 50/50 split; after any left-dock swap, move the TUI's
/// interior edge to the ratio already computed by `open_plan`.
#[cfg(unix)]
fn resize_direct_spawn(pane_id: &str, dock_right: bool, ratio: f64) {
    let amount = (ratio - 0.5).abs();
    if amount < 0.005 {
        return;
    }
    let direction = if dock_right { "right" } else { "left" };
    let _ = ipc::call_text(
        "pane.resize",
        serde_json::json!({
            "pane_id": pane_id,
            "direction": direction,
            "amount": amount,
        }),
    );
}

/// Grow the freshly-opened explorer into a full-height edge column. When the
/// tab's chosen edge was already split vertically, the explorer only gets the
/// top slot; each repair step re-parents the pane below it as a down-split of
/// the pane beside it. herdr no-ops same-tab moves, so each step bounces the
/// pane through a temporary tab (herdr auto-closes it once emptied).
/// Best-effort: any miss just leaves the layout as it was.
fn full_height_repair(pane_id: &str, dock_right: bool) {
    for _ in 0..4 {
        let Ok(layout) = ipc::call_text("pane.layout", serde_json::json!({ "pane_id": pane_id }))
        else {
            return;
        };
        let Some(step) = launch::repair_step(&layout, pane_id, dock_right) else {
            return;
        };
        let bounced = ipc::call_text(
            "pane.move",
            serde_json::json!({
                "pane_id": step.below,
                "destination": { "type": "new_tab" },
                "focus": false,
            }),
        );
        if bounced.is_err() {
            return;
        }
        let _ = ipc::call_text(
            "pane.move",
            serde_json::json!({
                "pane_id": step.below,
                "destination": {
                    "type": "tab",
                    "tab_id": step.tab,
                    "target_pane_id": step.beside,
                    "split": "down",
                },
                "focus": false,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snooze_set_clear_and_sweep() {
        let dir = std::env::temp_dir().join(format!("aa-ft-snooze-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        snooze::set(&dir, "w1:t1");
        snooze::set(&dir, "w1:t2");
        assert!(snooze::is_set(&dir, "w1:t1"));
        assert!(!snooze::is_set(&dir, "w1:t9"));
        assert!(!snooze::is_set(&dir, ""), "empty tab id never snoozes");

        snooze::clear(&dir, "w1:t1");
        assert!(!snooze::is_set(&dir, "w1:t1"));

        // Sweep drops markers for tabs that no longer exist.
        let live = std::collections::BTreeSet::from(["w1:t3".to_string()]);
        snooze::sweep(&dir, &live);
        assert!(!snooze::is_set(&dir, "w1:t2"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discrete_tab_creation_and_manual_toggles_wait_for_the_lock() {
        assert!(must_wait_for_lock(false, r#"{"event":"tab_created"}"#));
        assert!(must_wait_for_lock(false, r#"{"event":"tab.created"}"#));
        assert!(must_wait_for_lock(true, ""));
        assert!(!must_wait_for_lock(false, r#"{"event":"tab_focused"}"#));
    }

    #[test]
    fn direct_activities_use_the_unified_pane_and_stable_keys() {
        for (target, value, key) in [
            (Target::Explorer, "explorer", "f9"),
            (Target::Search, "search", "f10"),
            (Target::SourceControl, "source-control", "f11"),
            (Target::QuickOpen, "quick-open", "f12"),
        ] {
            assert_eq!(Target::from_env_value(value), Some(target));
            assert_eq!(target.env_value(), value);
            assert_eq!(target.key(), key);
            assert_eq!(target.pane_view(true), View::Explorer);
        }
        assert_eq!(Target::SourceControl.pane_view(false), View::SourceControl);
        assert_eq!(Target::SourceControl.initial_view(), View::SourceControl);
        assert_eq!(Target::from_env_value("unknown"), None);
    }
}

fn must_wait_for_lock(explicit: bool, event_json: &str) -> bool {
    explicit || launch::event_kind(event_json) == "tab_created"
}
