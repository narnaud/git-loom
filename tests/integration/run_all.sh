#!/usr/bin/env bash
# Run all integration tests and print a summary.
#
# Usage:
#   ./tests/integration/run_all.sh
#   GL_BIN=target/release/git-loom ./tests/integration/run_all.sh

set -uo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors
if [[ -t 1 ]]; then
    RED='\033[0;31m'; GREEN='\033[0;32m'; BOLD='\033[1m'; NC='\033[0m'
else
    RED=''; GREEN=''; BOLD=''; NC=''
fi

passed=0
failed=0
failed_names=()

for test_script in "$DIR"/test_*.sh; do
    name="$(basename "$test_script" .sh)"
    printf "${BOLD}── %s ${NC}\n" "$name"
    if bash "$test_script"; then
        ((passed++)) || true
    else
        ((failed++)) || true
        failed_names+=("$name")
    fi
done

echo ""
printf "${BOLD}────────────────────────────────${NC}\n"
total=$((passed + failed))
if [[ $failed -eq 0 ]]; then
    printf "${GREEN}${BOLD}$passed/$total passed${NC}\n"
else
    printf "${RED}${BOLD}$passed/$total passed${NC} — failed: ${failed_names[*]}\n"
    exit 1
fi
