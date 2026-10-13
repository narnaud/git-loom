# Demo videos

Sources for `docs/src/assets/git-loom-cli.mp4` and `git-loom-tui.mp4`.

Re-render (Git Bash, from this folder; needs vhs + ttyd + ffmpeg, e.g. `scoop install vhs ttyd`):

```sh
(cd ../.. && cargo build --release) && mkdir -p bin && cp ../../target/release/git-loom.exe bin/
(cd fake-gh && cargo build --release) && cp fake-gh/target/release/fake-gh.exe bin/gh.exe
python -m venv venv && venv/Scripts/python -m pip install pywinpty
bash setup.sh cli && vhs cli.tape
bash setup.sh tui && vhs tui.tape
```

- setup.sh rebuilds the fake "taskflow" repo under work/<mode>; dates are pinned so short IDs are stable.
- The remote looks like git@github.com:acme/taskflow.git but fake-ssh.sh serves it from work/<mode>/origin.git,
  and bin/gh.exe (fake-gh) answers loom's gh calls offline. GH_TOKEN is a dummy, so a real gh can't act.
- CLI split screen: pane.sh redraws the right pane after each command with pane.py, which runs
  `git loom` in a ConPTY (loom only colors a terminal) and draws it from column PANE_X.
