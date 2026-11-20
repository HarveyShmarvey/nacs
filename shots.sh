#!/bin/sh
# shots.sh — publish pictures from target/shots so a PR can show them.
# ./shots.sh plate-opened walk-on-path    prints one markdown image line per name.
# The pictures go to the `shots` branch (pictures only, no code, never merged),
# under a folder named for the current branch, so the tree stays text.
set -eu
cd "$(dirname "$0")"
[ $# -gt 0 ] || { echo "usage: ./shots.sh <name>...   (names of target/shots/<name>.png)" >&2; exit 2; }
branch=$(git rev-parse --abbrev-ref HEAD)
dir=$(echo "$branch" | tr '/' '-')
url=$(git remote get-url origin | sed -e 's|^git@github.com:|https://github.com/|' -e 's|\.git$||')
GIT_INDEX_FILE="$(git rev-parse --git-dir)/shots-index"
export GIT_INDEX_FILE
rm -f "$GIT_INDEX_FILE"
parent=""
if git fetch -q origin shots 2>/dev/null; then
    parent=$(git rev-parse FETCH_HEAD)
    git read-tree "$parent"
fi
for name in "$@"; do
    file="target/shots/$name.png"
    [ -f "$file" ] || { echo "no $file — run the shell test that takes it" >&2; exit 1; }
    blob=$(git hash-object -w "$file")
    git update-index --add --cacheinfo "100644,$blob,$dir/$name.png"
done
tree=$(git write-tree)
commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} -m "shots: $branch")
rm -f "$GIT_INDEX_FILE"
git push -q origin "$commit:refs/heads/shots"
for name in "$@"; do
    echo "![$name]($url/blob/$commit/$dir/$name.png?raw=true)"
done
