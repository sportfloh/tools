#!/usr/bin/env bash

TOOL_CMD=$(jq -r '.tool_input.command // ""' 2>/dev/null) || exit 0
echo "$TOOL_CMD" | grep -qE 'git.+push' || exit 0

BRANCH=$(git -C /home/user/tools rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
case "$BRANCH" in ""|main|HEAD) exit 0;; esac

EXISTING=$(GH_REPO=sportfloh/tools gh pr list --head "$BRANCH" --json number --jq 'length' 2>/dev/null) || true
[ "${EXISTING:-0}" -gt 0 ] && exit 0

TITLE=$(git -C /home/user/tools log -1 --format='%s' HEAD 2>/dev/null) || true
GH_REPO=sportfloh/tools gh pr create \
  --title "${TITLE:-Changes on $BRANCH}" \
  --body "" \
  --base main \
  --head "$BRANCH" 2>&1 || true
