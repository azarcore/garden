#!/bin/sh
# Publish the garden planner: build the release app and push only the built
# files to the GitHub Pages repo (default azarcore/garden-planner).
#
#   scripts/publish.sh             build, then publish
#   scripts/publish.sh --dry-run   build and prepare the commit, but don't push
#
# Your source code is never pushed. The publishing commit uses your GitHub
# no-reply address, so your own email stays private. Needs: trunk, git, and the
# GitHub CLI (gh) signed in.
set -eu

REPO="${PUBLISH_REPO:-azarcore/garden-planner}"
DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    *) echo "publish: unknown option $arg" >&2; exit 2 ;;
  esac
done

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# Publish only what's committed, so the live app matches a known commit.
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "publish: you have uncommitted changes; commit them first so the live app matches a commit" >&2
  exit 1
fi
COMMIT=$(git rev-parse --short HEAD)

echo "publish: building the release app from $COMMIT…"
(cd crates/garden-web && trunk build --release)
DIST="$ROOT/crates/garden-web/dist"
[ -f "$DIST/index.html" ] && [ -f "$DIST/cache-list.js" ] || {
  echo "publish: the build didn't produce index.html and cache-list.js" >&2
  exit 1
}

# The GitHub identity for the commit: your no-reply address, not your email.
ID=$(gh api user --jq .id)
LOGIN=$(gh api user --jq .login)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
echo "publish: fetching $REPO…"
gh repo clone "$REPO" "$WORK/site" -- --depth 1 --quiet

cd "$WORK/site"
# Replace everything with the new build (git history keeps the old ones).
find . -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
cp -R "$DIST/." .
rm -f .trunk-lock
touch .nojekyll # serve the files as they are, without Jekyll

git add -A
if git diff --cached --quiet; then
  echo "publish: the live app is already this build; nothing to publish"
  exit 0
fi
git -c user.name="$LOGIN" -c user.email="$ID+$LOGIN@users.noreply.github.com" \
  commit --quiet -m "Publish Garden Planner (built from $COMMIT)"
git --no-pager show --stat --format='publish: %s' HEAD | head -20

URL="https://$(echo "$REPO" | cut -d/ -f1).github.io/$(echo "$REPO" | cut -d/ -f2)/"
if [ "$DRY_RUN" -eq 1 ]; then
  echo "publish: dry run, nothing pushed. Would publish to $URL"
  exit 0
fi
git push --quiet origin HEAD
echo "publish: pushed. GitHub Pages updates $URL within a minute or two."
echo "publish: phones pick up the new version the next time the app starts."
