#!/bin/sh
# lock.sh — the merge gate: both feature configs, the headless shell tests, a smoke run.
# CI runs these same legs on every push, in parallel, on Linux and Windows; that run is the gate.
# Run this by hand only when there is no CI to ask (offline, or before the first push of a new repo).
set -eu
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"
echo "1/4 cargo test (with the window)"
cargo test
echo "2/4 cargo test --no-default-features (engine alone)"
cargo test --no-default-features
echo "3/4 shell tests (the window driven headless)"
# One thread on Linux: its Vulkan loader crashes now and then when several tests draw at once.
threads=""; [ "$(uname)" = Linux ] && threads="--test-threads=1"
cargo test --features shell-tests --test shell_tests -- $threads
echo "4/4 smoke run"
cargo run --quiet -- plate.cam > /dev/null
echo "ALL CHECKS PASSED"
