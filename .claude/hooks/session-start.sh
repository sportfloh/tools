#!/usr/bin/env bash
# Provisions a Claude Code on the web container with every tool referenced in
# CLAUDE.md. Idempotent: anything already present is skipped.
#
#   sync  : Rust toolchain (wasm32 + rustfmt + clippy), cargo-binstall, trunk,
#           wasm-pack, cargo-edit, ast-grep, fd, ripgrep, Chrome + matching
#           chromedriver for `wasm-pack test --headless --chrome`
#   async : sem + weave (no prebuilt binaries, ~10 min source build) — logged
#           to $LOG_DIR/sem-weave.log; git diff/merge drivers are wired up when done
set -euo pipefail

[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0

REPO="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../.." && pwd)}"
TOOLS_BIN=/opt/claude-tools/bin   # prepended to PATH, shadows /opt/node22/bin/chromedriver
LOG_DIR=/tmp/claude-session-start
mkdir -p "$TOOLS_BIN" "$LOG_DIR"
export PATH="$TOOLS_BIN:$HOME/.cargo/bin:$PATH"

have() { command -v "$1" >/dev/null 2>&1; }
log() { echo "session-start: $*" >&2; }

apt_install() {
    have apt-get || return 1
    if [ -z "${APT_UPDATED:-}" ]; then
        apt-get update -qq >/dev/null 2>&1 || true
        APT_UPDATED=1
    fi
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "$@" >/dev/null 2>&1
}

# Persist PATH for the session's Bash commands.
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    echo "export PATH=\"$TOOLS_BIN:\$HOME/.cargo/bin:\$PATH\"" >> "$CLAUDE_ENV_FILE"
fi

# --- Rust toolchain (channel + target come from rust-toolchain.toml) ----------
(cd "$REPO" && rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install stable)
(cd "$REPO" && rustup component add rustfmt clippy >/dev/null 2>&1 && rustup target add wasm32-unknown-unknown >/dev/null 2>&1)

# --- cargo-binstall ------------------------------------------------------------
if ! have cargo-binstall; then
    curl -sSfL https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
        | bash >/dev/null 2>&1
fi

# --- trunk (binstall's source fallback fails to compile lightningcss; use the release tarball)
if ! have trunk; then
    curl -sSfL https://github.com/trunk-rs/trunk/releases/latest/download/trunk-x86_64-unknown-linux-gnu.tar.gz \
        | tar -xz -C "$HOME/.cargo/bin" trunk
fi

# --- wasm-pack, cargo-edit (cargo upgrade) -------------------------------------
have wasm-pack || cargo binstall -y --quiet wasm-pack
have cargo-upgrade || cargo binstall -y --quiet cargo-edit

# --- ast-grep (/usr/bin/sg on Debian/Ubuntu is shadow-utils, not ast-grep) -------
if ! have ast-grep; then
    npm install -g --silent @ast-grep/cli >/dev/null 2>&1 || cargo binstall -y --quiet ast-grep
fi

# --- fd (Debian packages it as `fdfind`) ---------------------------------------
if ! have fd; then
    if ! have fdfind; then
        apt_install fd-find || cargo binstall -y --quiet fd-find
    fi
    have fdfind && ! have fd && ln -sf "$(command -v fdfind)" "$TOOLS_BIN/fd"
fi

# --- ripgrep -------------------------------------------------------------------
have rg || apt_install ripgrep || cargo binstall -y --quiet ripgrep

# --- Chrome + chromedriver matching its major version ----------------------------
# Playwright's Chromium is preinstalled; the chromedriver npm ships in /opt/node22/bin
# usually targets a newer Chrome, so fetch the exact Chrome-for-Testing driver.
CHROME="$(ls -d "${PLAYWRIGHT_BROWSERS_PATH:-/opt/pw-browsers}"/chromium-*/chrome-linux/chrome 2>/dev/null | sort -V | tail -1 || true)"
if [ -n "$CHROME" ]; then
    ln -sf "$CHROME" "$TOOLS_BIN/google-chrome"
    CHROME_VER="$("$CHROME" --version | rg -o '[0-9]+(\.[0-9]+){3}')"
    DRIVER_VER="$("$TOOLS_BIN/chromedriver" --version 2>/dev/null | rg -o '[0-9]+(\.[0-9]+){3}' || true)"
    if [ "$CHROME_VER" != "$DRIVER_VER" ]; then
        tmp="$(mktemp -d)"
        if curl -sSfLo "$tmp/cd.zip" \
            "https://storage.googleapis.com/chrome-for-testing-public/$CHROME_VER/linux64/chromedriver-linux64.zip"; then
            python3 -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$tmp/cd.zip" "$tmp"
            install -m755 "$tmp/chromedriver-linux64/chromedriver" "$TOOLS_BIN/chromedriver"
        else
            log "no chromedriver $CHROME_VER on Chrome for Testing; WASM tests may fail"
        fi
        rm -rf "$tmp"
    fi
fi

# --- sem + weave (source build, in the background) --------------------------------
configure_sem_weave() {
    git -C "$REPO" config merge.weave.name "Entity-level semantic merge"
    git -C "$REPO" config merge.weave.driver "$(command -v weave-driver) %O %A %B %L %P"
    sem telemetry off >/dev/null 2>&1 || true
    sem setup >/dev/null 2>&1 || true
}
if have sem && have weave && have weave-driver; then
    configure_sem_weave
else
    (
        cargo binstall -y --quiet sem-cli weave-cli weave-driver && configure_sem_weave
    ) > "$LOG_DIR/sem-weave.log" 2>&1 &
    disown
    log "building sem + weave in the background (log: $LOG_DIR/sem-weave.log)"
fi

exit 0
