#!/usr/bin/env bash
# Installs the code search tools referenced in CLAUDE.md (ast-grep, fd, ripgrep)
# in Claude Code on the web sessions. Idempotent: skips anything already present.
set -euo pipefail

[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0

have() { command -v "$1" >/dev/null 2>&1; }

apt_install() {
    have apt-get || return 1
    if [ -z "${APT_UPDATED:-}" ]; then
        apt-get update -qq >/dev/null 2>&1 || true
        APT_UPDATED=1
    fi
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "$@" >/dev/null 2>&1
}

# ast-grep (note: /usr/bin/sg on Debian/Ubuntu is shadow-utils, not ast-grep)
if ! have ast-grep; then
    npm install -g --silent @ast-grep/cli >/dev/null 2>&1 \
        || cargo install ast-grep --locked --quiet
fi

# fd (Debian packages it as `fdfind`)
if ! have fd; then
    if ! have fdfind; then
        apt_install fd-find || cargo install fd-find --locked --quiet
    fi
    if have fdfind && ! have fd; then
        ln -sf "$(command -v fdfind)" /usr/local/bin/fd
    fi
fi

# ripgrep
if ! have rg; then
    apt_install ripgrep || cargo install ripgrep --locked --quiet
fi

exit 0
