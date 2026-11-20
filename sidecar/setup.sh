#!/usr/bin/env sh
# sidecar/setup.sh — the sidecar's one-time setup (Phase 50 slice 10): a
# Python 3.11 venv under sidecar/.venv and the pinned stack from
# requirements.txt (about 1.5 GB, from wheels — no compiler). This is the
# only network use the sidecar ever makes, and it is yours, once.
# Run from the repo root:  sh sidecar/setup.sh
set -e
here="$(cd "$(dirname "$0")" && pwd)"
venv="$here/.venv"
python=""
for candidate in python3.11 python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 && "$candidate" --version 2>&1 | grep -q "3\.11"; then
        python="$candidate"
        break
    fi
done
if [ -z "$python" ]; then
    echo "Python 3.11 is needed (the pinned stack installs from wheels there) — install it, then run this again." >&2
    exit 1
fi
if [ ! -x "$venv/bin/python" ]; then
    echo "creating $venv with $python"
    "$python" -m venv "$venv"
fi
"$venv/bin/python" -m pip install --upgrade pip
"$venv/bin/pip" install -r "$here/requirements.txt"
echo
echo "done — the window imports a dropped STEP file now, and so does:  nacs --import <model.step> <project.cam>"
