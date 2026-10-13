# Sourced (hidden) at the start of each tape: isolated, deterministic git env.
# vhs starts bash in `D:/...`, whose colon would split the PATH entry below.
DEMO=$(cygpath -u "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)")
export PATH="$DEMO/bin:$PATH"
export GIT_CONFIG_GLOBAL="$DEMO/gitconfig"
export GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME="Sam Rivera" GIT_AUTHOR_EMAIL="sam@taskflow.dev"
export GIT_COMMITTER_NAME="Sam Rivera" GIT_COMMITTER_EMAIL="sam@taskflow.dev"
export GIT_AUTHOR_DATE="2026-10-12T14:00:00+0200" GIT_COMMITTER_DATE="2026-10-12T14:00:00+0200"
export GIT_EDITOR="$DEMO/editor.sh"
export LESS=-FRX
# Push goes to the fake ssh + gh stub only; a dummy token keeps any real gh logged out.
export GH_TOKEN=demo-not-a-real-token
# Short and neutral: loom prints worktree paths, and they show in the video.
export DEMO_WORK=${DEMO_WORK:-/c/demo}
export FAKE_GH_STATE="$DEMO_WORK/gh-state"
export PANE_STATE="$DEMO_WORK/pane-state"
export DEMO_PY="$DEMO/venv/Scripts/python.exe"
export PS1='\[\e[1;35m\]\W\[\e[0m\] \$ '
