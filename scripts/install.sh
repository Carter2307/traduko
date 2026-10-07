#!/bin/bash
# Builds Traduko, makes Traduko.app and installs it in ~/Applications.
#
#   scripts/install.sh            build and install
#   scripts/install.sh --launch   the same, then open the app
#
# The translation models are not inside the bundle: they live in
# ~/Library/Application Support/Traduko/models, fetched once by fetch-models.sh.
# A model inside the bundle would be signed and checked again by macOS at
# every build, which costs several seconds each time.
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="Traduko"
BUNDLE_ID="com.github.carter2307.traduko"
MODELS_DIR="${TRADUKO_MODELS_DIR:-$HOME/Library/Application Support/$APP_NAME/models}"

# A model set is complete when both directions hold their five files: the
# same rule the app applies. An interrupted fetch is fetched again.
have_set() {
  local direction file
  for direction in fr-en en-fr; do
    for file in model.safetensors config.json source.spm target.spm vocab.json; do
      [ -f "$MODELS_DIR/$1/$direction/$file" ] || return 1
    done
  done
}
if ! have_set light && ! have_set accurate; then
  echo "==> No complete model set yet: fetching"
  /bin/bash scripts/fetch-models.sh
fi

# The installed copy cannot start beside another one (a development run,
# for example): it would exit at once and never register at login.
if [ "${1:-}" = "--launch" ]; then
  others="$(ps -xo pid=,comm= | awk -v keep="$HOME/Applications/$APP_NAME.app/Contents/MacOS/traduko" \
    '{ pid = $1; sub(/^ *[0-9]+ /, ""); n = split($0, part, "/"); if (part[n] == "traduko" && $0 != keep) print pid }' | tr '\n' ' ')"
  if [ -n "$others" ]; then
    echo "install: another copy of Traduko is running (pid $others): quit it first." >&2
    exit 1
  fi
fi

echo "==> cargo build --release"
cargo build --release --locked -p traduko
version="$(cargo metadata --no-deps --format-version 1 | plutil -extract packages.0.version raw -o - -)"

args=(--name "$APP_NAME" --bundle-id "$BUNDLE_ID"
  --binary target/release/traduko --icon assets/icon-1024.png
  --version "$version" --build "$(date +%Y%m%d%H%M%S)"
  --out target/bundle --install "$HOME/Applications")
[ "${1:-}" = "--launch" ] && args+=(--launch)
/bin/bash scripts/make-app.sh "${args[@]}"
