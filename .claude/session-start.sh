#!/bin/sh
# Runs when a Claude Code session starts (.claude/settings.json). In a cloud
# session it runs ../setup.sh so the box can build and test; anywhere else it
# does nothing. The full output goes to target/setup.log, one line to the session.
[ "${CLAUDE_CODE_REMOTE:-}" = true ] || exit 0
cd "$(dirname "$0")/.."
mkdir -p target
if sh setup.sh > target/setup.log 2>&1; then
    echo "setup.sh ran clean (target/setup.log); cargo is in ~/.cargo/bin"
else
    echo "setup.sh FAILED — read target/setup.log before building"
fi
[ -n "${CLAUDE_ENV_FILE:-}" ] && echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> "$CLAUDE_ENV_FILE"
exit 0
