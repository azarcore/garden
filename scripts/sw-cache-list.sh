#!/bin/sh
# Write cache-list.js next to sw.js: every file the app needs offline, and a
# version that changes whenever any of them changes (the 3D bundle included).
# Run by Trunk after the build, after build-3d.sh (see crates/garden-web/Trunk.toml).
set -eu

DIR="${TRUNK_STAGING_DIR:-$(cd "$(dirname "$0")/.." && pwd)/crates/garden-web/dist}"
cd "$DIR"

LIST=$(find . -type f ! -name sw.js ! -name cache-list.js ! -name '.*' | sed 's|^\./||' | LC_ALL=C sort)
VERSION=$(for f in $LIST; do printf '%s:%s\n' "$f" "$(shasum -a 256 "$f" | cut -d' ' -f1)"; done | shasum -a 256 | cut -c1-16)

quote() { printf '"%s"' "$1"; }
app=""
lazy=""
for f in $LIST; do
  case "$f" in
    garden3d/*) lazy="${lazy:+$lazy,}$(quote "$f")" ;;
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
