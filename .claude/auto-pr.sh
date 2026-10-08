#!/usr/bin/env bash
# PostToolUse hook: after a `git push` from a feature branch, open a PR
# against main if the branch has none yet. Needs the gh CLI; silently does
# nothing without it (e.g. in cloud sessions).
command -v gh >/dev/null 2>&1 || exit 0
REPO_DIR="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/.." && pwd)}"
TOOL_CMD=$(jq -r '.tool_input.command // ""' 2>/dev/null) || exit 0
echo "$TOOL_CMD" | grep -qE 'git.+push' || exit 0
BRANCH=$(git -C "$REPO_DIR" rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
case "$BRANCH" in ""|main|HEAD) exit 0;; esac
# owner/repo from the origin URL (https://host/owner/repo(.git), git@host:owner/repo.git,
# or a proxy URL ending in /owner/repo).
ORIGIN=$(git -C "$REPO_DIR" remote get-url origin 2>/dev/null) || exit 0
REPO=$(echo "${ORIGIN%.git}" | tr ':' '/' | awk -F/ '{ print $(NF-1) "/" $NF }')
case "$REPO" in */*) ;; *) exit 0;; esac
export GH_REPO="$REPO"
EXISTING=$(gh pr list --head "$BRANCH" --json number --jq 'length' 2>/dev/null) || true
[ "${EXISTING:-0}" -gt 0 ] && exit 0
TITLE=$(git -C "$REPO_DIR" log -1 --format='%s' HEAD 2>/dev/null) || true
gh pr create --title "${TITLE:-Changes on $BRANCH}" --body "" --base main --head "$BRANCH" 2>&1 || true
