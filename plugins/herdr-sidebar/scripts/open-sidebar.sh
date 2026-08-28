#!/usr/bin/env bash
# open-explorer.sh — unix launcher for the herdr-sidebar explorer pane.
#
# Idempotent "launch-or-focus, toggle on repeat", scoped to the current tab:
#   - no Explorer pane in the current tab       -> open at the configured dock edge
#   - an Explorer pane exists but isn't focused -> focus it
#   - the focused pane IS the Explorer pane     -> close it (toggle off)
#
# Left docking splits the leftmost pane and swaps into its narrow slot. Right
# docking splits the rightmost pane with the inverse original-pane ratio and
# needs no swap. The unit-tested --open-plan output owns that choice.
#
# All ids/ratios come from the binary's unit-tested stdin modes
# (--launch-decision / --focused-pane / --open-plan), never ad-hoc JSON parsing;
# the ids it emits are validated flag-safe before reaching an argv.
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
bin_dir="$script_dir/../target/release"
bin="$bin_dir/herdr-sidebar"

# Without the binary there is no decision logic; fall back to herdr's declarative
# pane open (right split, not left-docked — degraded but functional).
if [ ! -x "$bin" ]; then
  exec "$herdr_bin" plugin pane open \
    --plugin herdr-sidebar \
    --entrypoint filetree \
    --placement split \
    --direction right \
    --focus
fi

# Serialize with the auto-ensure hook (SAME lock dir): a user toggle racing a
# focus-burst ensure otherwise docks TWO sidebars (seen live on the first
# macOS install). Unlike the hook, the launcher WAITS for the lock rather
# than yielding, so the user's toggle is never silently dropped.
lock_dir="${TMPDIR:-/tmp}/herdr-sidebar-ensure.lock"
locked=""
for _ in $(seq 1 20); do
  if mkdir "$lock_dir" 2>/dev/null; then locked=1; break; fi
  sleep 0.5
done
if [ -z "$locked" ]; then
  # Break locks older than 30s (a crashed holder), otherwise give up.
  now="$(date +%s)"
  born="$(stat -c %Y "$lock_dir" 2>/dev/null || stat -f %m "$lock_dir" 2>/dev/null || echo "$now")"
  [ "$((now - born))" -ge 30 ] || exit 0
  rm -rf "$lock_dir" 2>/dev/null
  mkdir "$lock_dir" 2>/dev/null || exit 0
fi
trap 'rmdir "$lock_dir" 2>/dev/null' EXIT

panes="$("$herdr_bin" pane list 2>/dev/null || true)"

open_pane() {
  local fp fid fcwd plan target ratio needs_swap out np
  fp="$(printf '%s' "$panes" | "$bin" --focused-pane 2>/dev/null || true)"
  fid="${fp%%	*}"
  fcwd="${fp#*	}"
  if [ -z "$fid" ]; then
    "$herdr_bin" plugin pane open --plugin herdr-sidebar \
      --entrypoint filetree --placement split --direction right --focus
  fi

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
  [ -n "$np" ] || exit 1

  # Left docking swaps into the narrow left slot; right docking is already
  # in place. PATH makes the same bare launch work in any configured shell.
  focus_open="$("$bin" --focus-on-open 2>/dev/null || echo on)"
  if [ "$needs_swap" = "true" ]; then
    "$herdr_bin" pane swap --source-pane "$np" --target-pane "$target" >/dev/null 2>&1 || true
    if [ "$focus_open" != "on" ] && [ "$target" = "$fid" ]; then
      # "Focus on open: off" (⚙ Settings): the sidebar docks in the
      # background, but the swap drags focus onto the explorer slot (focus
      # follows the slot). Hand it back right away, the ensure hook's move.
      "$herdr_bin" pane focus --direction right --pane "$np" >/dev/null 2>&1 || true
    fi
  fi
  "$herdr_bin" pane run "$np" "herdr-sidebar"
  "$herdr_bin" pane rename "$np" Explorer >/dev/null 2>&1 || true
  # Give the TUI time to stamp its identity token before hooks re-check.
  sleep 3
  if [ "$focus_open" = "on" ]; then
    # herdr has no focus-by-id; a zoom on/off cycle focuses deterministically.
    "$herdr_bin" pane zoom "$np" --on >/dev/null 2>&1 || true
    "$herdr_bin" pane zoom "$np" --off
  fi
}

decision="OPEN"
if [ -n "$panes" ]; then
  decision="$(printf '%s' "$panes" | "$bin" --launch-decision 2>/dev/null || echo OPEN)"
fi

# Snooze markers keep the auto-ensure hook from reopening a sidebar the user
# toggled closed; toggling open clears the tab's marker again. Hide/open also
# records a durable per-workspace preference (keyed by workspace label).
snooze_dir="${TMPDIR:-/tmp}/herdr-sidebar-snooze"
tab="$(printf '%s' "$panes" | "$bin" --focused-tab 2>/dev/null || true)"

remember_sidebar() {
  local vis=$1
  local ws_id
  ws_id="$(printf '%s' "$panes" | "$bin" --focused-workspace 2>/dev/null || true)"
  [ -n "$ws_id" ] || return 0
  local list
  list="$("$herdr_bin" workspace list 2>/dev/null || true)"
  printf '%s' "$list" | "$bin" --remember-sidebar "$vis" "$ws_id" >/dev/null 2>&1 || true
}

graceful_close() {
  local pane_id=$1 listed present acknowledged=""
  "$herdr_bin" pane send-keys "$pane_id" ctrl+q >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do
    sleep 0.1
    if listed="$("$herdr_bin" pane list 2>/dev/null)" &&
      present="$(printf '%s' "$listed" | "$bin" --pane-has-token "$pane_id" 2>/dev/null)"
    then
      if [ "$present" != "yes" ]; then acknowledged=1; break; fi
    fi
  done
  if [ -n "$acknowledged" ]; then
    "$herdr_bin" pane close "$pane_id" >/dev/null 2>&1 || true
    return 0
  fi
  "$herdr_bin" notification show "Sidebar close cancelled" \
    --body "The pane did not acknowledge a safe shutdown; try again shortly." \
    --position bottom-right --sound none >/dev/null 2>&1 || true
  return 1
}

case "$decision" in
  "FOCUS "*)
    pid="${decision#FOCUS }"
    "$herdr_bin" pane zoom "$pid" --on >/dev/null 2>&1 || true
    "$herdr_bin" pane zoom "$pid" --off
    remember_sidebar on
    ;;
  "CLOSE "*)
    pid="${decision#CLOSE }"
    if graceful_close "$pid"; then
      if [ -n "$tab" ]; then
        mkdir -p "$snooze_dir" 2>/dev/null
        : > "$snooze_dir/${tab//:/_}"
      fi
      remember_sidebar off
    fi
    ;;
  "REPLACE "*)
    # Dead pane (stale heartbeat): close the corpse, then dock a fresh one.
    pid="${decision#REPLACE }"
    "$herdr_bin" pane close "$pid" >/dev/null 2>&1 || true
    panes="$("$herdr_bin" pane list 2>/dev/null || true)"
    open_pane
    remember_sidebar on
    ;;
  *)
    [ -n "$tab" ] && rm -f "$snooze_dir/${tab//:/_}" 2>/dev/null
    open_pane
    remember_sidebar on
    ;;
esac
