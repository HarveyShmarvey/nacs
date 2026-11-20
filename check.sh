#!/bin/sh
# check.sh — the quick loop: one test run and a smoke run. Use ./lock.sh before a merge.
set -eu
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"
cargo test
cargo run --quiet -- plate.cam > /dev/null
echo "CHECK PASSED"
