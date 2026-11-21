#!/bin/sh
# setup.sh — make a fresh Linux box (a cloud session, WSL, a new machine) able to run ./lock.sh.
# Installs the desktop headers the window builds against, a software GPU for the
# shell tests, the compiler rust-toolchain.toml pins, and the sidecar's Python
# (through uv, which it installs for this user when the box has no python3.11).
# Safe to run again: each step is skipped when its result is already there.
# Windows needs none of this beyond rustup and `sh sidecar/setup.sh`.
set -eu
cd "$(dirname "$0")"
[ "$(uname)" = Linux ] || { echo "setup.sh is for Linux; on Windows install rustup and run: sh sidecar/setup.sh"; exit 0; }

sudo=""; [ "$(id -u)" = 0 ] || sudo="sudo"
packages="build-essential pkg-config curl ca-certificates libxkbcommon-dev libwayland-dev libx11-dev libxcb1-dev libgl1-mesa-dev mesa-vulkan-drivers libxrender1 libxext6"
missing=""
for p in $packages; do dpkg -s "$p" >/dev/null 2>&1 || missing="$missing $p"; done
if [ -n "$missing" ]; then
    echo "setup: apt packages:$missing"
    $sudo apt-get update -q
    DEBIAN_FRONTEND=noninteractive $sudo apt-get install -y -q $missing
fi

export PATH="$HOME/.cargo/bin:$PATH"
if ! command -v rustup >/dev/null 2>&1; then
    echo "setup: rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none --profile minimal
fi
rustup toolchain install   # the one rust-toolchain.toml names
cargo fetch --locked

# The sidecar wants Python 3.11 exactly (its pinned stack installs from wheels there).
if [ ! -x sidecar/.venv/bin/python ]; then
    echo "setup: the sidecar's Python (about 1.5 GB)"
    if command -v python3.11 >/dev/null 2>&1; then
        sh sidecar/setup.sh
    else
        # No python3.11 (stock Ubuntu 24.04 ships 3.12): uv brings its own.
        # Its installer puts it in ~/.local/bin, for this user only.
        export PATH="$HOME/.local/bin:$PATH"
        if ! command -v uv >/dev/null 2>&1; then
            echo "setup: uv (no python3.11 here)"
            curl -LsSf https://astral.sh/uv/install.sh | sh
        fi
        # Fail here: carrying on would end in "done" with no sidecar, and the
        # session-start hook would report a clean setup.
        command -v uv >/dev/null 2>&1 || { echo "setup: no python3.11, and uv could not be installed, so there is no sidecar. Install either one and run this again." >&2; exit 1; }
        uv venv --python 3.11 --seed sidecar/.venv
        uv pip install --python sidecar/.venv/bin/python -r sidecar/requirements.txt
    fi
fi
# The CAD library loads desktop libraries; say so here, not in a failed test.
[ ! -x sidecar/.venv/bin/python ] || sidecar/.venv/bin/python -c "import cadquery" || { echo "setup: the sidecar's Python cannot load cadquery (a system library is missing, named above)" >&2; exit 1; }
echo "setup: done — cargo test, ./lock.sh"
