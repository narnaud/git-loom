# keycast.py burns the keys in and writes ../src/assets/git-loom-tui.mp4.
Output ../../target/demo/git-loom-tui-raw.mp4

Set Shell "bash"
Set FontSize 20
Set Width 1920
Set Height 1080
Set Padding 32
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms
Set Framerate 20
Set CursorBlink false

Hide
Sleep 1500ms
Enter
Sleep 500ms
Type "source ./env.sh && cd $DEMO_WORK/tui/taskflow && clear"
Enter
Sleep 1s
Show

Type "git loom tui"
Sleep 400ms
Enter
Sleep 2500ms

# ── Browse: every row has its diff ──
Down
Sleep 700ms
Down
Sleep 700ms
Down
Sleep 400ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 350ms
Down
Sleep 900ms
Right
Sleep 1200ms
Left
Sleep 600ms

# ── Stack a branch on a commit ──
Type "b"
Sleep 800ms
Type "search-api"
Sleep 500ms
Enter
Sleep 2s

# ── Move a commit to the branch it belongs to ──
Up
Sleep 300ms
Up
Sleep 700ms
Type "m"
Sleep 1s
Up
Sleep 700ms
Up
Sleep 1s
Enter
Sleep 2s

# ── Uncommit one file of a commit ──
Right
Sleep 1s
Down
Sleep 700ms
Type "f"
Sleep 1s
Up
Sleep 400ms
Up
Sleep 1s
Enter
Sleep 2s

# ── Uncommit a whole commit ──
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 700ms
Type "f"
Sleep 1s
Up
Sleep 1s
Enter
Sleep 2s

# ── Amend the search fix into its commit ──
Down
Sleep 300ms
Down
Sleep 700ms
Type "f"
Sleep 1s
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 1s
Enter
Sleep 2s

# ── New branch, then commit one hunk into it ──
Down
Sleep 700ms
Type "b"
Sleep 800ms
Type "validation"
Sleep 500ms
Enter
Sleep 1500ms
Type "C"
Sleep 1500ms
Down
Sleep 700ms
Tab
Sleep 700ms
Space
Sleep 1200ms
Enter
Sleep 1500ms
Down
Sleep 1200ms
Enter
Sleep 2500ms

# ── Every action is a plain loom command ──
Type "L"
Sleep 4s
Type "L"
Sleep 800ms
Type "?"
Sleep 5s
