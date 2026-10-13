Output ../src/assets/git-loom-tui.mp4

Set Shell "bash"
Set FontSize 22
Set Width 1920
Set Height 1080
Set Padding 32
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms
Set Framerate 10
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
Sleep 440ms
Enter
Sleep 3300ms

# ── Browse: every row has its diff ──
Down
Sleep 1320ms
Down
Sleep 1650ms
Down
Sleep 1320ms
Down
Sleep 1100ms
Down
Sleep 1650ms
Right
Sleep 1100ms
Down
Sleep 1100ms
Down
Sleep 1650ms
Left
Sleep 1100ms

# ── Fold the search fix into the commit that wrote the bug ──
Up
Sleep 440ms
Up
Sleep 440ms
Up
Sleep 1100ms
Type "f"
Sleep 1980ms
Down
Sleep 1100ms
Down
Sleep 1980ms
Enter
Sleep 3300ms

# ── New branch, then commit a file straight into it ──
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 1100ms
Type "b"
Sleep 1320ms
Type "task-priority"
Sleep 660ms
Enter
Sleep 2750ms
Up
Sleep 330ms
Up
Sleep 1100ms
Type "c"
Sleep 1980ms
Down
Sleep 150ms
Down
Sleep 1320ms
Down
Sleep 1320ms
Up
Sleep 1320ms
Up
Sleep 1980ms
Enter
Sleep 3850ms

# ── Every action is a plain loom command ──
Type "L"
Sleep 4400ms
Type "L"
Sleep 880ms
Type "?"
Sleep 3850ms
Type "?"
Sleep 1100ms
Type "q"
Sleep 1100ms
