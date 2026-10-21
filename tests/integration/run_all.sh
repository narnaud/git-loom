#!/usr/bin/env bash
# Run all integration tests and print a summary.
#
# Usage:
#   ./tests/integration/run_all.sh
#   GL_BIN=target/release/git-loom ./tests/integration/run_all.sh
#   LOOM_TEST_JOBS=1 ./tests/integration/run_all.sh   # one script at a time, output live
#
# The scripts run concurrently by default (each keeps to its own temp dirs)
# and their output is printed in name order as they finish.

set -uo pipefail
unset CDPATH  # keep cd from echoing the directory into $(...)

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors
if [[ -t 1 ]]; then
    RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; BOLD='\033[1m'; NC='\033[0m'
else
    RED=''; GREEN=''; YELLOW=''; BOLD=''; NC=''
fi

scripts=("$DIR"/test_*.sh)
passed=0
failed=0
failed_names=()

record() {
    if [[ "$2" -eq 0 ]]; then
        ((passed++)) || true
    else
        ((failed++)) || true
        failed_names+=("$1")
    fi
}

if [[ "${LOOM_TEST_JOBS:-}" == 1 ]]; then
    for test_script in "${scripts[@]}"; do
        name="$(basename "$test_script" .sh)"
        printf "${YELLOW}${BOLD}── %s ${NC}\n" "$name"
        bash "$test_script"
        record "$name" $?
    done
else
    logdir="$(mktemp -d "${TMPDIR:-/tmp}/git-loom-run.XXXXXXXXXX")"
    trap 'rm -rf "$logdir"' EXIT
    pids=()
    for i in "${!scripts[@]}"; do
        (
            bash "${scripts[$i]}" > "$logdir/$i.log" 2>&1
            echo $? > "$logdir/$i.code"
        ) &
        pids[$i]=$!
    done
    for i in "${!scripts[@]}"; do
        name="$(basename "${scripts[$i]}" .sh)"
        wait "${pids[$i]}"
        printf "${YELLOW}${BOLD}── %s ${NC}\n" "$name"
        cat "$logdir/$i.log"
        record "$name" "$(cat "$logdir/$i.code" 2>/dev/null || echo 1)"
    done
fi

echo ""
printf "${BOLD}────────────────────────────────${NC}\n"
total=$((passed + failed))
if [[ $failed -eq 0 ]]; then
    printf "${GREEN}${BOLD}$passed/$total passed${NC}\n"
else
    printf "${RED}${BOLD}$passed/$total passed${NC} — failed: ${failed_names[*]}\n"
    exit 1
fi
