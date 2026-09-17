#!/bin/bash

set -euo pipefail

# Fail if any commit in BASE_SHA..HEAD_SHA is unsigned (`git log %G?` is `N`).
# Checks signature presence only; authenticity is not verified.
# Run from the repository root with a checkout that contains the full range:
#
#   ./ci/check-signed-commits.sh <base_sha> <head_sha>

BASE_SHA="${1:-}"
HEAD_SHA="${2:-}"

if [ -z "$BASE_SHA" ] || [ -z "$HEAD_SHA" ]; then
    echo "Usage: $0 <base_sha> <head_sha>" >&2
    exit 1
fi

COMMIT_SIGNATURES=$(git log --format="%H %G?" "${BASE_SHA}..${HEAD_SHA}" --)

UNSIGNED=0
while IFS=' ' read -r commit status; do
    if [ "$status" = "N" ]; then
        echo "Commit $commit is not signed."
        UNSIGNED=$((UNSIGNED + 1))
    fi
done <<< "$COMMIT_SIGNATURES"

if [ "$UNSIGNED" -gt 0 ]; then
    echo "Error: $UNSIGNED commit(s) are not signed. See CONTRIBUTING.md." >&2
    exit 1
fi

echo "All commits are signed."
