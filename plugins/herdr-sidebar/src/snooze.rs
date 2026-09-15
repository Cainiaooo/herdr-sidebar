//! Per-tab "the user closed/hid the sidebar here" markers: hide (« or b) and
//! the toggle CLOSE write one, the quiet ensure hook honors it — otherwise the
//! very next focus event would reopen what the user just closed. Toggle OPEN
//! clears it. Markers for tabs that no longer exist are swept each ensure run
//! (tab ids can be recycled).

use std::path::PathBuf;

pub fn dir() -> PathBuf {
    std::env::temp_dir().join("herdr-sidebar-snooze")
}

fn marker(dir: &std::path::Path, tab: &str) -> PathBuf {
    dir.join(tab.replace(':', "_"))
}

pub fn set(dir: &std::path::Path, tab: &str) {
    if !tab.is_empty() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(marker(dir, tab), b"");
    }
}

pub fn clear(dir: &std::path::Path, tab: &str) {
    if !tab.is_empty() {
        let _ = std::fs::remove_file(marker(dir, tab));
    }
}

pub fn is_set(dir: &std::path::Path, tab: &str) -> bool {
    !tab.is_empty() && marker(dir, tab).exists()
}

/// Hide this sidebar pane: snooze its tab, remember the workspace as
/// "don't auto-dock", then close the pane. Shared by Explorer and Source
/// Control so `b` / `«` cannot drift.
pub fn hide_pane(pane_id: &str) {
    if let Ok(panes) = crate::ipc::call_text("pane.list", serde_json::json!({})) {
        set(&dir(), &crate::launch::tab_of(&panes, pane_id));
        let list = crate::ipc::call_text("workspace.list", serde_json::json!({}))
            .unwrap_or_default();
        crate::state::remember_visible_for_pane(&panes, &list, pane_id, false);
    }
    let _ = crate::ipc::call_text(
        "pane.close",
        serde_json::json!({ "pane_id": pane_id }),
    );
}

pub fn sweep(dir: &std::path::Path, live_tabs: &std::collections::BTreeSet<String>) {
    let live: std::collections::BTreeSet<String> =
        live_tabs.iter().map(|t| t.replace(':', "_")).collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !live.contains(&entry.file_name().to_string_lossy().into_owned()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
