#!/bin/sh
# Write cache-list.js next to sw.js: every file the app needs offline, and a
# version that changes whenever any of them changes (the 3D bundle included).
# Run by Trunk after the build, after build-3d.sh (see crates/loma-web/Trunk.toml).
set -eu

DIR="${TRUNK_STAGING_DIR:-$(cd "$(dirname "$0")/.." && pwd)/crates/loma-web/dist}"
cd "$DIR"

if command -v shasum >/dev/null 2>&1; then
  sha() { shasum -a 256 "$@"; }
elif command -v sha256sum >/dev/null 2>&1; then
  sha() { sha256sum "$@"; }
else
  echo "sw-cache-list: needs shasum or sha256sum" >&2
  exit 1
fi

LIST=$(find . -type f ! -name sw.js ! -name cache-list.js ! -name '.*' | sed 's|^\./||' | LC_ALL=C sort)
VERSION=$(for f in $LIST; do printf '%s:%s\n' "$f" "$(sha "$f" | cut -d' ' -f1)"; done | sha | cut -c1-16)
if [ ${#VERSION} -ne 16 ]; then
  echo "sw-cache-list: could not compute a version" >&2
  exit 1
fi

quote() { printf '"%s"' "$1"; }
app=""
lazy=""
for f in $LIST; do
  case "$f" in
    loma3d/*) lazy="${lazy:+$lazy,}$(quote "$f")" ;;
    *) app="${app:+$app,}$(quote "$f")" ;;
  esac
done

cat > cache-list.js <<JS
// Written at build time by scripts/sw-cache-list.sh. Do not edit.
self.CACHE_VERSION = "$VERSION";
self.APP_FILES = [$app];
self.LAZY_FILES = [$lazy];
JS
echo "sw-cache-list: version $VERSION"
