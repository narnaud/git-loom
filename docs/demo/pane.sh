# Sourced by the CLI tape after env.sh: live status pane on the right of the terminal.
PANE_X=${PANE_X:-70}
__loom_pane() {
  PYTHONIOENCODING=utf-8 "$DEMO_PY" "$DEMO/pane.py" "$COLUMNS" "$LINES" "$PANE_X"
}
__loom_prompt() {
  # Comments change nothing: skip the redraw. Otherwise draw in the background,
  # after readline has drawn the prompt, whose clear-to-eol would blank a pane row.
  [[ $(history 1) =~ ^\ *[0-9]+\ +# ]] || ( __loom_pane & )
}

shopt -s checkwinsize
PROMPT_COMMAND=__loom_prompt
bind -x '"\C-l": clear; ( __loom_pane & )'
