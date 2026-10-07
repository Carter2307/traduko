#!/bin/bash
# Removes Traduko and everything it left behind.
#
#   scripts/uninstall.sh          prints what it would do (dry run, the default)
#   scripts/uninstall.sh --yes    does it
#
# Order matters: the login item must be removed BY THE APP while its bundle still
# exists (only the app itself can call SMAppService.unregister), so step 1 runs the
# app's own "disable" command before anything is deleted.
set -euo pipefail

APP_NAME="${APP_NAME:-Traduko}"
BUNDLE_ID="${BUNDLE_ID:-com.github.carter2307.traduko}"
BIN_NAME="${BIN_NAME:-traduko}"
AGENT_LABEL="${AGENT_LABEL:-$BUNDLE_ID.login}"
# What the app understands as "turn launch at login off, then exit".
UNREGISTER_ARGS="${UNREGISTER_ARGS:---login-item disable}"
HOME_DIR="${HOME_DIR:-$HOME}"                   # overridable for tests
LAUNCHCTL="${LAUNCHCTL:-/bin/launchctl}"        # overridable for tests
APPS_DIRS=("$HOME_DIR/Applications" "/Applications")

DO=0
[ "${1:-}" = "--yes" ] && DO=1
run() {
  if [ "$DO" = 1 ]; then echo "  run: $*"; "$@" || echo "  (failed, continuing)"; else echo "  would run: $*"; fi
}
remove() {
  if [ -e "$1" ]; then run rm -rf "$1"; else echo "  absent: $1"; fi
}
# True when the bundle at $1 is this app: another "Traduko.app" is left alone.
ours() {
  [ "$(plutil -extract CFBundleIdentifier raw -o - "$1/Contents/Info.plist" 2>/dev/null)" = "$BUNDLE_ID" ]
}

echo "1. Launch at login"
for dir in "${APPS_DIRS[@]}"; do
  ours "$dir/$APP_NAME.app" || continue
  exe="$dir/$APP_NAME.app/Contents/MacOS/$BIN_NAME"
  if [ -x "$exe" ]; then
    # shellcheck disable=SC2086
    run "$exe" $UNREGISTER_ARGS
  fi
done
# The fallback LaunchAgent: unload it (this also stops the app if launchd started
# it), then delete the plist.
run "$LAUNCHCTL" bootout "gui/$(id -u)/$AGENT_LABEL"
remove "$HOME_DIR/Library/LaunchAgents/$AGENT_LABEL.plist"

echo "2. Running copies"
for dir in "${APPS_DIRS[@]}"; do
  ours "$dir/$APP_NAME.app" || continue
  exe="$dir/$APP_NAME.app/Contents/MacOS/$BIN_NAME"
  pids="$(ps -axo pid=,comm= | awk -v exe="$exe" '{ pid = $1; sub(/^ *[0-9]+ /, ""); if ($0 == exe) print pid }')"
  for pid in $pids; do run kill "$pid"; done
done

echo "3. The app"
for dir in "${APPS_DIRS[@]}"; do
  if ours "$dir/$APP_NAME.app"; then remove "$dir/$APP_NAME.app"; else echo "  not Traduko, or absent: $dir/$APP_NAME.app"; fi
done

echo "4. Its data"
remove "$HOME_DIR/Library/Application Support/$APP_NAME"            # models, settings, history, instance.lock
remove "$HOME_DIR/Library/Caches/$BUNDLE_ID"
remove "$HOME_DIR/Library/HTTPStorages/$BUNDLE_ID"
remove "$HOME_DIR/Library/Preferences/$BUNDLE_ID.plist"
remove "$HOME_DIR/Library/Saved Application State/$BUNDLE_ID.savedState"
remove "$HOME_DIR/Library/Logs/$APP_NAME"

echo "5. By hand, if they exist"
echo "  - System Settings > General > Login Items & Extensions: a leftover '$APP_NAME' row"
echo "    (it appears when the bundle was deleted before step 1 could run); select it and click '-'."
echo "  - Models fetched into the shared Hugging Face cache: ~/.cache/huggingface/hub/models--<org>--<name>"
[ "$DO" = 1 ] || echo "Dry run: nothing was changed. Run again with --yes to apply."
