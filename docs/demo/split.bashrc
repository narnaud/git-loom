# rcfile of the bash inside split.py: tells it, at each prompt, whether to show the
# status pane and for which directory. `export LOOM_PANE=on` opens the pane.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
__loom_n=0
__loom_state() {
  printf '%s %s %s\n' "${LOOM_PANE:-off}" "$((++__loom_n))" "$(pwd -W)" > "$PANE_STATE"
}
PROMPT_COMMAND=__loom_state
