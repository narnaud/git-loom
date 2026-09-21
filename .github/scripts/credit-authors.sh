#!/usr/bin/env bash
# Append " by <author>" to every changelog entry for a commit in <base>...<head>.
# Credited entries no longer end in a commit link, so re-running is a no-op.
# Usage: credit-authors.sh <base> <head> <file>...   (needs GH_TOKEN, GH_REPO)
set -euo pipefail

base=$1
head=$2
shift 2

# Raw "<sha> <author>" lines, not @tsv: a name is taken verbatim, escapes and
# all, and a Git author name can hold no newline to break the format.
authors=$(mktemp)
gh api --paginate "repos/$GH_REPO/compare/$base...$head" \
  --jq '.commits[]
        | .sha + " " + (if .author.login then "@" + .author.login else .commit.author.name end)' \
  > "$authors"

for file in "$@"; do
  awk '
    NR == FNR { author[$1] = substr($0, index($0, " ") + 1); next }
    {
      if (match($0, /\/commit\/[0-9a-f]+\)\)$/)) {
        sha = substr($0, RSTART + 8, RLENGTH - 10)
        if (sha in author) $0 = $0 " by " author[sha]
      }
      print
    }' "$authors" "$file" > "$file.credited"
  mv "$file.credited" "$file"
done
