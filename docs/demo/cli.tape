Output ../src/assets/git-loom-cli.mp4

Set Shell "bash"
Set FontSize 22
Set Width 1920
Set Height 1080
Set Padding 40
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms
Set Framerate 20
Set CursorBlink false

Hide
Sleep 1500ms
Enter
Sleep 500ms
Type "source ./env.sh && source $DEMO/pane.sh && cd $DEMO/work/cli/taskflow"
Enter
Sleep 1s
Ctrl+L
Sleep 2s
Show

# ── Intro ──
Sleep 1s
Type "# git-loom: all your feature branches, woven into"
Enter
Type "# one integration branch. Live status on the right."
Enter
Sleep 3s

# ── Commit to any branch ──
Hide
Type "$DEMO/scene.sh darkmode-edit"
Enter
Sleep 1s
Ctrl+L
Sleep 2s
Show
Sleep 1.5s
Type "# Commit straight to a new branch, no checkout"
Enter
Type 'git loom commit -b dark-mode -m "Add dark mode toggle" zz'
Sleep 400ms
Enter
Sleep 4s

# ── Hunk selector ──
Hide
Type "$DEMO/scene.sh hunks"
Enter
Sleep 1s
Ctrl+L
Sleep 2s
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
Sleep 2s
Show
Sleep 2s
Type "# fold, the swiss army knife. Amend any commit:"
Enter
Type "git loom fold st npm"
Sleep 400ms
Enter
Sleep 3.5s
Type "# Committed to the wrong branch? Move it:"
Enter
Type 'git loom commit -b fs -m "Load the theme script" in'
Sleep 400ms
Enter
Sleep 2.5s
Type "git loom fold kst dm"
Sleep 400ms
Enter
Sleep 3.5s
Type "# Not ready after all? Uncommit it:"
Enter
Type "git loom fold kst zz"
Sleep 400ms
Enter
Sleep 4s

# ── Update ──
Hide
Type "$DEMO/scene.sh teammate"
Enter
Sleep 2s
Ctrl+L
Sleep 2s
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
Type "git loom branch search-api -t yvk"
Sleep 400ms
Enter
Sleep 4s
Type "# Push the stack: one PR per branch, chained"
Enter
Type "git loom push fs"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 1.5s
Type "# cargo install git-loom"
Sleep 4s
