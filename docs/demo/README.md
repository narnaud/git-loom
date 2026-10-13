# Demo videos

Sources for `docs/src/assets/git-loom-cli.mp4` and `git-loom-tui.mp4`.

Re-render (Git Bash, from this folder; needs vhs + ttyd + ffmpeg, e.g. `scoop install vhs ttyd`):

```sh
(cd ../.. && cargo build --release) && mkdir -p bin && cp ../../target/release/git-loom.exe bin/
(cd fake-gh && cargo build --release) && cp fake-gh/target/release/fake-gh.exe bin/gh.exe
python -m venv venv && venv/Scripts/python -m pip install pywinpty pyte
bash setup.sh cli && vhs cli.tape
bash setup.sh tui && vhs tui.tape && python keycast.py tui.tape ../../target/demo/git-loom-tui-raw.mp4 ../src/assets/git-loom-tui.mp4
```

- setup.sh rebuilds the fake "taskflow" repo under `$DEMO_WORK/<mode>` (default `C:/demo`, kept short
  because loom prints worktree paths); dates are pinned so short IDs are stable.
- The remote looks like git@github.com:acme/taskflow.git but fake-ssh.sh serves it from
  `$DEMO_WORK/<mode>/origin.git`, and bin/gh.exe (fake-gh) answers loom's gh calls offline.
  GH_TOKEN is a dummy, so a real gh can't act.
- CLI split screen: split.py runs bash in its own pseudo-console, emulated with pyte, beside a
  `git loom status` pane that redraws at each prompt (split.bashrc). `export LOOM_PANE=on` opens it.
- TUI keys: keycast.py times each key the tape presses and burns it into the video as a keycap.
