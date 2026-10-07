#!/bin/bash
# make-app.sh: turns a cargo release binary into Name.app, signed ad hoc.
#
#   make-app.sh --name "Mascot" --bundle-id dev.example.mascot \
#               --binary target/release/mascot --icon assets/icon-1024.png \
#               [--version 0.1.0] [--build 1] [--min-macos 13.0] \
#               [--resource PATH]... [--framework DYLIB]... \
#               [--plist-string KEY=VALUE]... \
#               [--out dist] [--sign -] [--hardened] [--dock] \
#               [--install ~/Applications] [--launch]
#
# Needs only what macOS ships: sips, iconutil, plutil, codesign, ditto, xattr.
# Runs with the system bash (3.2).
#
# What it does, in order:
#   1. builds Name.app/Contents/{MacOS,Resources[,Frameworks]} in a staging folder
#   2. writes Info.plist (LSUIElement = true unless --dock)
#   3. builds AppIcon.icns from the 1024 px PNG (sips + iconutil)
#   4. copies resources (APFS clones, so large files cost no time and no space)
#   5. signs inside-out: dylibs first, then the bundle (ad hoc by default)
#   6. verifies: plutil -lint, codesign --verify --deep --strict, codesign -dv
#   7. with --install DIR: swaps the bundle into DIR (the old one is kept until
#      the new one is in place); with --launch: opens it
set -euo pipefail

die() { echo "make-app: $*" >&2; exit 1; }
step() { echo "==> $*"; }

NAME="" BUNDLE_ID="" BINARY="" ICON="" EXEC_NAME=""
VERSION="0.1.0" BUILD="1" MIN_MACOS="13.0"
OUT_DIR="dist" SIGN_ID="-" HARDENED=0 AGENT_APP=1
CATEGORY="public.app-category.productivity"
INSTALL_DIR="" LAUNCH=0
RESOURCES=() FRAMEWORKS=() PLIST_STRINGS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --name) NAME="$2"; shift 2 ;;
    --bundle-id) BUNDLE_ID="$2"; shift 2 ;;
    --binary) BINARY="$2"; shift 2 ;;
    --icon) ICON="$2"; shift 2 ;;
    --exec-name) EXEC_NAME="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --build) BUILD="$2"; shift 2 ;;
    --min-macos) MIN_MACOS="$2"; shift 2 ;;
    --category) CATEGORY="$2"; shift 2 ;;
    --resource) RESOURCES+=("$2"); shift 2 ;;
    --framework) FRAMEWORKS+=("$2"); shift 2 ;;
    --plist-string) PLIST_STRINGS+=("$2"); shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --sign) SIGN_ID="$2"; shift 2 ;;
    --hardened) HARDENED=1; shift ;;
    --dock) AGENT_APP=0; shift ;;
    --install) INSTALL_DIR="$2"; shift 2 ;;
    --launch) LAUNCH=1; shift ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

[ -n "$NAME" ] || die "--name is required"
[ -n "$BUNDLE_ID" ] || die "--bundle-id is required"
[ -f "$BINARY" ] || die "--binary: no such file: $BINARY"
[ -f "$ICON" ] || die "--icon: no such file: $ICON"
[ -n "$EXEC_NAME" ] || EXEC_NAME="$(basename "$BINARY")"
case "$BUNDLE_ID" in
  *[!A-Za-z0-9.-]*) die "--bundle-id may hold only letters, digits, '.' and '-': $BUNDLE_ID" ;;
esac
case "$NAME" in
  */*) die "--name must not contain '/'" ;;
esac
for tool in sips iconutil plutil codesign ditto xattr; do
  command -v "$tool" >/dev/null || die "missing tool: $tool"
done
file "$BINARY" | grep -q "Mach-O" || die "--binary is not a Mach-O file: $BINARY"

icon_width=$(sips -g pixelWidth "$ICON" | awk '/pixelWidth/ {print $2}')
icon_height=$(sips -g pixelHeight "$ICON" | awk '/pixelHeight/ {print $2}')
icon_format=$(sips -g format "$ICON" | awk '/format/ {print $2}')
[ "$icon_format" = "png" ] || die "--icon must be a PNG (found: $icon_format)"
[ "$icon_width" = "1024" ] && [ "$icon_height" = "1024" ] ||
  die "--icon must be 1024 x 1024 (found: ${icon_width} x ${icon_height})"

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
APP="$OUT_DIR/$NAME.app"
WORK="$(mktemp -d "$OUT_DIR/.make-app.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

step "Staging $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/$EXEC_NAME"
chmod 755 "$APP/Contents/MacOS/$EXEC_NAME"
printf 'APPL????' > "$APP/Contents/PkgInfo"

step "Writing Info.plist"
PLIST="$APP/Contents/Info.plist"
plutil -create xml1 "$PLIST"
put_string() { plutil -insert "$1" -string "$2" "$PLIST"; }
put_bool() { plutil -insert "$1" -bool "$2" "$PLIST"; }
put_string CFBundleIdentifier "$BUNDLE_ID"
put_string CFBundleName "$NAME"
put_string CFBundleDisplayName "$NAME"
put_string CFBundleExecutable "$EXEC_NAME"
put_string CFBundleIconFile "AppIcon"
put_string CFBundlePackageType "APPL"
put_string CFBundleInfoDictionaryVersion "6.0"
put_string CFBundleDevelopmentRegion "en"
put_string CFBundleShortVersionString "$VERSION"
put_string CFBundleVersion "$BUILD"
put_string LSMinimumSystemVersion "$MIN_MACOS"
put_string LSApplicationCategoryType "$CATEGORY"
put_string NSPrincipalClass "NSApplication"
put_bool NSHighResolutionCapable true
if [ "$AGENT_APP" = 1 ]; then
  # No Dock icon, no menu bar, not in Cmd-Tab. The app can still open windows.
  put_bool LSUIElement true
fi
for pair in ${PLIST_STRINGS[@]+"${PLIST_STRINGS[@]}"}; do
  case "$pair" in
    *=*) put_string "${pair%%=*}" "${pair#*=}" ;;
    *) die "--plist-string wants KEY=VALUE, got: $pair" ;;
  esac
done
plutil -lint "$PLIST" >/dev/null || die "Info.plist does not pass plutil -lint"

step "Building AppIcon.icns from $(basename "$ICON")"
ICONSET="$WORK/AppIcon.iconset"
mkdir "$ICONSET"
for size in 16 32 128 256 512; do
  double=$((size * 2))
  sips -z "$size" "$size" "$ICON" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z "$double" "$double" "$ICON" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

# cp -c clones on APFS (instant, no extra space) and falls back to a real copy.
clone() { cp -cR "$1" "$2" 2>/dev/null || cp -R "$1" "$2"; }

for item in ${RESOURCES[@]+"${RESOURCES[@]}"}; do
  [ -e "$item" ] || die "--resource: no such file or folder: $item"
  step "Resource: $(basename "$item") ($(du -sh "$item" | awk '{print $1}'))"
  clone "$item" "$APP/Contents/Resources/"
done

if [ ${#FRAMEWORKS[@]} -gt 0 ]; then
  mkdir -p "$APP/Contents/Frameworks"
  for lib in "${FRAMEWORKS[@]}"; do
    [ -e "$lib" ] || die "--framework: no such file: $lib"
    step "Library: $(basename "$lib")"
    clone "$lib" "$APP/Contents/Frameworks/"
  done
fi

# Finder info and resource forks make codesign fail ("detritus not allowed").
xattr -cr "$APP"

step "Signing (identity: $SIGN_ID)"
SIGN_OPTS=(--force --sign "$SIGN_ID")
if [ "$SIGN_ID" = "-" ]; then
  # An ad hoc signature cannot carry a secure timestamp.
  SIGN_OPTS+=(--timestamp=none)
fi
if [ "$HARDENED" = 1 ]; then
  # Off by default: under the hardened runtime, library validation refuses dylibs
  # that are not signed by the same Team ID, and an ad hoc signature has none.
  SIGN_OPTS+=(--options runtime)
fi
# Inside-out: nested code first, the bundle last. (`codesign --deep` does the same
# walk, but `man codesign` marks it "DEPRECATED for signing as of macOS 13.0".)
if [ -d "$APP/Contents/Frameworks" ]; then
  find "$APP/Contents/Frameworks" -type f \( -name '*.dylib' -o -perm -u+x \) -print0 |
    while IFS= read -r -d '' nested; do
      codesign "${SIGN_OPTS[@]}" "$nested"
    done
fi
TIMEFORMAT='    codesign took %R s'
time codesign "${SIGN_OPTS[@]}" "$APP"

step "Verifying"
plutil -lint "$PLIST"
TIMEFORMAT='    verify took %R s'
time codesign --verify --deep --strict --verbose=2 "$APP"
codesign -dv --verbose=2 "$APP" 2>&1 | grep -E '^(Identifier|Format|CodeDirectory|Signature|TeamIdentifier|Info.plist|Sealed Resources|CDHash)' | sed 's/^/    /'
signed_id=$(codesign -dv "$APP" 2>&1 | awk -F= '/^Identifier=/ {print $2}')
[ "$signed_id" = "$BUNDLE_ID" ] || die "signature identifier is '$signed_id', expected '$BUNDLE_ID'"
if xattr -r "$APP" 2>/dev/null | grep -q com.apple.quarantine; then
  die "the bundle carries com.apple.quarantine; Gatekeeper would assess it"
fi
# Gatekeeper's verdict, for information. An ad hoc signature is always "rejected"
# here; that only matters for a copy that carries the quarantine attribute
# (downloaded, AirDropped), never for a bundle built on this Mac.
echo "    spctl: $(spctl --assess --type execute -vv "$APP" 2>&1 | tr '\n' ' ' || true)"
echo "    size: $(du -sh "$APP" | awk '{print $1}')"

if [ -n "$INSTALL_DIR" ]; then
  mkdir -p "$INSTALL_DIR"
  DEST="$(cd "$INSTALL_DIR" && pwd)/$NAME.app"
  step "Installing $DEST"
  # Ask a running copy of that exact bundle to quit (SIGTERM, then 5 s of grace).
  running_pids() {
    ps -axo pid=,comm= | awk -v exe="$DEST/Contents/MacOS/$EXEC_NAME" \
      '{ pid = $1; sub(/^ *[0-9]+ /, ""); if ($0 == exe) print pid }'
  }
  pids="$(running_pids)"
  if [ -n "$pids" ]; then
    # shellcheck disable=SC2086
    kill $pids 2>/dev/null || true
    for _ in $(seq 50); do
      [ -z "$(running_pids)" ] && break
      sleep 0.1
    done
  fi
  rm -rf "$DEST.new" "$DEST.old"
  # ditto keeps the signature, and clones on APFS.
  ditto --clone "$APP" "$DEST.new" 2>/dev/null || ditto "$APP" "$DEST.new"
  if [ -e "$DEST" ]; then mv "$DEST" "$DEST.old"; fi
  if ! mv "$DEST.new" "$DEST"; then
    if [ -e "$DEST.old" ]; then mv "$DEST.old" "$DEST"; fi
    die "could not move the new bundle into place; the old one is kept"
  fi
  rm -rf "$DEST.old"
  codesign --verify --deep --strict "$DEST" || die "the installed copy fails verification"
  echo "    installed: $DEST"
  # A copy that was running is started again: an update must not leave the
  # mascot gone from the desktop.
  if [ "$LAUNCH" = 1 ] || [ -n "$pids" ]; then
    step "Launching"
    open "$DEST"
  fi
fi

step "Done: $APP"
