#!/bin/sh
# Build the 3D preview bundle (crates/garden-3d) and put it next to the app as
# garden3d/garden_3d.js + garden3d/garden_3d_bg.wasm. Run by Trunk after each
# build (see crates/garden-web/Trunk.toml); the app downloads it only when
# someone opens the 3D view.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

if [ "${TRUNK_PROFILE:-debug}" = "release" ]; then
  PROFILE=release-3d
else
  PROFILE=dev-3d
fi
OUT="${TRUNK_STAGING_DIR:-$ROOT/crates/garden-web/dist}/garden3d"

# wasm-bindgen's CLI must match the library version in Cargo.lock exactly.
WANT=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$ROOT/Cargo.lock")
# Print the first of PATH's copy and Trunk's cached copies of tool $1 for which
# `$1 --version` matches $3 (empty = any version); $2 = Trunk cache folder pattern.
find_tool() {
  for f in "$(command -v "$1" 2>/dev/null || true)" \
    "$HOME/Library/Caches/dev.trunkrs.trunk"/$2/"$1" \
    "$HOME/Library/Caches/dev.trunkrs.trunk"/$2/bin/"$1" \
    "${XDG_CACHE_HOME:-$HOME/.cache}/trunk"/$2/"$1" \
    "${XDG_CACHE_HOME:-$HOME/.cache}/trunk"/$2/bin/"$1"; do
    [ -n "$f" ] && [ -x "$f" ] || continue
    if [ -z "$3" ] || "$f" --version 2>/dev/null | grep -q " $3\$"; then
      echo "$f"
      return
    fi
  done
}
BINDGEN=$(find_tool wasm-bindgen "wasm-bindgen-$WANT" "$WANT")
if [ -z "$BINDGEN" ]; then
  echo "build-3d: needs wasm-bindgen $WANT (cargo install wasm-bindgen-cli --version $WANT)" >&2
  exit 1
fi

cargo build --manifest-path "$ROOT/Cargo.toml" -p garden-3d --features viewer \
  --target wasm32-unknown-unknown --profile "$PROFILE"

TARGET_DIR=$(cargo metadata --manifest-path "$ROOT/Cargo.toml" --format-version 1 --no-deps \
  | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
mkdir -p "$OUT"
"$BINDGEN" --target web --no-typescript --out-dir "$OUT" \
  "$TARGET_DIR/wasm32-unknown-unknown/$PROFILE/garden_3d.wasm"

if [ "$PROFILE" = release-3d ]; then
  OPT=$(find_tool wasm-opt "wasm-opt-version_*" "")
  if [ -z "$OPT" ]; then
    echo "build-3d: wasm-opt not found; the 3D bundle is not size-optimised" >&2
  else
    "$OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
      -o "$OUT/garden_3d_bg.wasm" "$OUT/garden_3d_bg.wasm"
  fi
fi
