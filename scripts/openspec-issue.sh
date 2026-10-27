#!/usr/bin/env bash
# Validates an OpenSpec change and opens it as a GitHub issue.
#
#   scripts/openspec-issue.sh <change-id> [feature|bug|change] [--exploration notes.md] [--dry-run]
#
# Needs the openspec CLI and, unless --dry-run, the GitHub CLI (gh) signed in.
set -euo pipefail

usage() { sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
[ $# -ge 1 ] || usage
change=$1; shift
type=feature; exploration=""; dry_run=0
while [ $# -gt 0 ]; do
  case $1 in
    feature|bug|change) type=$1 ;;
    --exploration) exploration=$2; shift ;;
    --dry-run) dry_run=1 ;;
    *) usage ;;
  esac
  shift
done

cd "$(git rev-parse --show-toplevel)"
openspec validate "$change" --strict --no-interactive

body=$(mktemp); trap 'rm -f "$body"' EXIT
python3 scripts/openspec_issue.py build "$change" ${exploration:+--exploration "$exploration"} > "$body"

# The proposal's first line of "Why" makes a readable title.
summary=$(awk '/^## Why/{getline; while ($0 ~ /^[[:space:]]*$/) getline; print; exit}' "openspec/changes/$change/proposal.md" | cut -c1-90)
title="[$type] ${summary:-$change}"

if [ $dry_run -eq 1 ]; then
  echo "Title: $title"; echo; cat "$body"; exit 0
fi
[ -n "$exploration" ] || echo "Tip: pass --exploration notes.md to include your /opsx:explore findings; you can also edit the issue afterwards." >&2
gh issue create --title "$title" --label "$type" --label openspec --body-file "$body"
