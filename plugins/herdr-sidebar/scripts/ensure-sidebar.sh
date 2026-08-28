#!/usr/bin/env bash
# ensure-explorer.sh — unix [[events]] hook body: make sure the FOCUSED tab has
# an Explorer pane docked on the configured edge, WITHOUT stealing focus.
#
# Runs on pane/tab lifecycle events, so it must be idempotent and quiet:
# already present → exit; else open unfocused (see ensure-explorer.ps1 for the
# focus-follows-the-slot rationale behind the final `pane focus`).
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
bin_dir="$script_dir/../target/release"
bin="$bin_dir/herdr-sidebar"
[ -x "$bin" ] || exit 0

# Per-workspace hide/toggle outranks ⚙ Settings → Auto-open sidebar.
# Unknown spaces follow the global default (issue #8). Do not exit on
# "off" before launch-decision: a hidden space can still hold a label-only
# corpse that must be REPLACE-closed (Windows sidecar already does this).
# `herdr workspace list` already prints JSON; `--json` is rejected on 0.8.2.
scope="$("$bin" --event-scope 2>/dev/null || true)"
ws_id="${scope%%:*}"
ws_list="$("$herdr_bin" workspace list 2>/dev/null || true)"
if [ -n "$ws_id" ]; then
  should="$(printf '%s' "$ws_list" | "$bin" --should-auto-open "$ws_id" 2>/dev/null || echo on)"
else
  should="$("$bin" --auto-open 2>/dev/null || echo on)"
fi

# Focus events arrive in bursts and concurrent ensures each open an explorer —
# serialize with an atomic mkdir lock. Focus events may skip a held lock because
# another event will follow; tab.created is discrete and must wait or the new
# preview tab can permanently miss its sidebar (issue #32).
lock_dir="${TMPDIR:-/tmp}/herdr-sidebar-ensure.lock"
event_kind="$("$bin" --event-kind 2>/dev/null || true)"
if ! mkdir "$lock_dir" 2>/dev/null; then
  # Break locks older than 30s (a crashed ensure).
  now="$(date +%s)"
  born="$(stat -c %Y "$lock_dir" 2>/dev/null || stat -f %m "$lock_dir" 2>/dev/null || echo "$now")"
  if [ $((now - born)) -gt 30 ]; then
    rm -rf "$lock_dir" 2>/dev/null
    mkdir "$lock_dir" 2>/dev/null || exit 0
  elif [ "$event_kind" = "tab_created" ]; then
    acquired="false"
    for _ in {1..20}; do
      sleep 0.5
      if mkdir "$lock_dir" 2>/dev/null; then
        acquired="true"
        break
      fi
    done
    [ "$acquired" = "true" ] || exit 0
  else
    exit 0
  fi
fi
trap 'rmdir "$lock_dir" 2>/dev/null' EXIT

# Snapshot AFTER acquiring the lock, so a just-finished ensure's rename is visible.
panes="$("$herdr_bin" pane list 2>/dev/null || true)"
[ -n "$panes" ] || exit 0

# The tab THIS event is about. The globally focused pane is still the space
# you came from during a workspace switch, which rooted new sidebars in the
# wrong project. Everything below reasons about this one scope — decision,
# snooze check, and spawn cwd must agree or we dock into the wrong tab.
# (`scope` was already read from the event payload before the lock.)

decision="$(printf '%s' "$panes" | "$bin" --launch-decision "" "$scope" 2>/dev/null || true)"
replacing="false"
case "$decision" in
  "OPEN") ;;
  "REPLACE "*)
    # A label-only or stale pane is a corpse, not a reason to suppress the
    # hook. Close it and refresh: focus/layout can change with the close.
    pid="${decision#REPLACE }"
    "$herdr_bin" pane close "$pid" >/dev/null 2>&1 || true
    panes="$("$herdr_bin" pane list 2>/dev/null || true)"
    [ -n "$panes" ] || exit 0
    replacing="true"
    ;;
  *) exit 0 ;;
esac

# After REPLACE cleanup, a hidden workspace must not redock. Live FOCUS/CLOSE
# already exited above; OPEN with should=off is a no-op too.
if [ "$should" = "off" ]; then
  exit 0
fi

# Respect a tab the user toggled closed (open-explorer.sh writes the marker) —
# otherwise the very next focus event would reopen what they just closed.
# pane.focused has no tab_id; --snooze-tab locates the pane's tab so a click
# on Perforce/shell in the same tab still honors hide.
snooze_dir="${TMPDIR:-/tmp}/herdr-sidebar-snooze"
tab="$(printf '%s' "$panes" | "$bin" --snooze-tab "$scope" 2>/dev/null || true)"
if [ "$replacing" != "true" ] && [ -n "$tab" ] && [ -f "$snooze_dir/${tab//:/_}" ]; then
  exit 0
fi

fp="$(printf '%s' "$panes" | "$bin" --focused-pane "$scope" 2>/dev/null || true)"
fid="${fp%%	*}"
fcwd="${fp#*	}"
[ -n "$fid" ] || exit 0

target="$fid"
if [ "$("$bin" --dock-right 2>/dev/null || echo left)" = "right" ]; then
  ratio="0.75"
  needs_swap="false"
else
  ratio="0.25"
  needs_swap="true"
fi
plan="$("$herdr_bin" pane layout --pane "$fid" 2>/dev/null | "$bin" --open-plan 2>/dev/null || true)"
if [ -n "$plan" ]; then
  IFS=$'\t' read -r target ratio needs_swap <<< "$plan"
fi

split_args=(pane split "$target" --direction right --ratio "$ratio" --no-focus \
  --env "PATH=$bin_dir${PATH:+:$PATH}")
[ -n "$fcwd" ] && split_args+=(--cwd "$fcwd")
[ -n "${HERDR_PLUGIN_STATE_DIR:-}" ] && \
  split_args+=(--env "HERDR_PLUGIN_STATE_DIR=$HERDR_PLUGIN_STATE_DIR")
out="$("$herdr_bin" "${split_args[@]}" 2>/dev/null || true)"
np="$(printf '%s' "$out" | sed -n 's/.*"pane_id":"\([^"]*\)".*/\1/p' | head -n1)"
[ -n "$np" ] || exit 0

if [ "$needs_swap" = "true" ]; then
  "$herdr_bin" pane swap --source-pane "$np" --target-pane "$target" >/dev/null 2>&1 || true
fi
"$herdr_bin" pane run "$np" "herdr-sidebar"
"$herdr_bin" pane rename "$np" Explorer >/dev/null 2>&1 || true

# Keep the ensure lock through the TUI's first identity stamp. The swap and
# focus calls above/below emit fresh focus events; without this wait, one can
# see the new label before its token, classify the pane as a resumed corpse,
# and replace it forever (issue #29). Six seconds matches the Windows ensure.
for _ in {1..30}; do
  current_panes="$("$herdr_bin" pane list 2>/dev/null || true)"
  if [ "$(printf '%s' "$current_panes" | "$bin" --pane-has-token "$np" 2>/dev/null || true)" = "yes" ]; then
    break
  fi
  sleep 0.2
done

# Hand focus back if the swap left it on the explorer (focus follows the slot).
if [ "$needs_swap" = "true" ] && [ "$target" = "$fid" ]; then
  "$herdr_bin" pane focus --direction right --pane "$np" >/dev/null 2>&1 || true
fi
exit 0
