Output ../src/assets/git-loom-cli.mp4

Set Shell "bash"
Set FontSize 20
Set Width 1920
Set Height 1080
Set Padding 32
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms
Set Framerate 20
Set CursorBlink false

Hide
Sleep 1500ms
Enter
Sleep 500ms
Type 'source ./env.sh && cd $DEMO_WORK/cli/taskflow && exec "$DEMO_PY" "$DEMO/split.py" 54 "$(cygpath -w "$BASH")" "$(cygpath -w "$DEMO/split.bashrc")"'
Enter
Sleep 3s
Ctrl+L
Sleep 1s
Show

# ── Intro ──
Sleep 1s
Type "# git-loom: all your feature branches, woven into one"
Enter
Type "# integration branch, and reshaped without leaving it."
Enter
Sleep 1.5s
Type "git loom init"
Sleep 400ms
Enter
Sleep 2.5s
Type "# Weave in a branch you already have"
Enter
Type "git loom merge feature-search"
Sleep 400ms
Enter
Sleep 2.5s
Type "# Every branch and commit, each with a short ID"
Enter
Type "git loom"
Sleep 400ms
Enter
Sleep 3.5s
Type "# For this demo, status stays pinned on the left"
Enter
Sleep 1.5s
Hide
Type "export LOOM_PANE=on"
Enter
Sleep 2.5s
Show
Sleep 1.5s

# ── Commit to any branch ──
Hide
Type "$DEMO/scene.sh darkmode-edit"
Enter
Sleep 1s
Ctrl+L
Sleep 1.5s
Show
Sleep 1.5s
Type "# Commit straight to a new branch, no checkout"
Enter
Type 'git loom commit zz -b dark-mode -m "Add dark mode toggle"'
Sleep 400ms
Enter
Sleep 4s

# ── Hunk selector ──
Hide
Type "$DEMO/scene.sh hunks"
Enter
Sleep 1s
Ctrl+L
Sleep 1.5s
Show
Sleep 2s
Type "# Two unrelated edits in routes.py: pick one hunk"
Enter
Type 'git loom commit -p -b validation -m "Validate task titles"'
Sleep 400ms
Enter
Sleep 2s
Down
Sleep 1.5s
Tab
Sleep 800ms
Space
Sleep 2s
Enter
Sleep 3.5s

# ── Absorb ──
Type "# The rest are fixes: absorb sends each hunk home"
Enter
Type "git loom absorb"
Sleep 400ms
Enter
Sleep 5s

# ── Fold ──
Hide
Type "$DEMO/scene.sh amend-css && $DEMO/scene.sh toggle-button"
Enter
Sleep 1s
Ctrl+L
Sleep 1.5s
Show
Sleep 2s
Type "# fold, the swiss army knife. Amend any commit:"
Enter
Type "git loom fold st ryq"
Sleep 400ms
Enter
Sleep 3.5s
Type "# Committed to the wrong branch? Move it:"
Enter
Type 'git loom commit in -b fs -m "Load the theme script"'
Sleep 400ms
Enter
Sleep 2.5s
Type "git loom fold tms dm"
Sleep 400ms
Enter
Sleep 3.5s
Type "# Not ready after all? Uncommit it:"
Enter
Type "git loom fold tms zz"
Sleep 400ms
Enter
Sleep 4s

# ── Reword ──
Hide
Ctrl+L
Sleep 1s
Show
Sleep 1s
Type "# Reword any commit, no interactive rebase"
Enter
Type 'git loom reword tvw -m "Reject empty task titles"'
Sleep 400ms
Enter
Sleep 4s

# ── Update ──
Hide
Type "$DEMO/scene.sh teammate"
Enter
Sleep 2s
Ctrl+L
Sleep 1.5s
Show
Sleep 1.5s
Type "# Teammates pushed: rebase every branch at once"
Enter
Type "git loom update"
Sleep 400ms
Enter
Sleep 4.5s

# ── Stacks and PRs ──
Type "# Stack branches: the search API under its UI"
Enter
Type "git loom branch search-api --on yvk"
Sleep 400ms
Enter
Sleep 4s
Type "# Push the stack: one PR per branch, chained"
Enter
Type "git loom push fs"
Sleep 400ms
Enter
Sleep 6s

# ── Worktrees ──
Hide
Ctrl+L
Sleep 1s
Show
Sleep 1s
Type "# Something else? A worktree, with its own integration branch"
Enter
Type "git loom wt new fix"
Sleep 400ms
Enter
Sleep 3s
Type "cd ../taskflow-fix"
Sleep 400ms
Enter
Sleep 3s
Type "git loom wt"
Sleep 400ms
Enter
Sleep 4.5s

# ── Outro ──
Hide
Type "cd ../taskflow"
Enter
Sleep 1.5s
Ctrl+L
Sleep 1s
Show
Sleep 1.5s
Type@30ms "# ...and much more:"
Enter
Type@30ms "#   split, swap, drop, diff, switch, continue / abort"
Enter
Type@30ms "#   git loom tui: all of it, in a full-screen UI"
Enter
Type@30ms "#   push to GitHub, GitLab, Azure DevOps or Gerrit"
Enter
Type@30ms "#   an agent mode for AI coding assistants"
Enter
Sleep 2s
Enter
Type "# cargo install git-loom"
Sleep 5s
