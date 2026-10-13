#!/bin/sh
# Stand-in editor for the TUI demo: writes the message only when the template is empty.
if ! grep -qv '^#' "$1" 2>/dev/null || [ -z "$(grep -v '^#' "$1" | tr -d '[:space:]')" ]; then
  printf '%s\n' "${DEMO_MSG:-Validate task titles}" > "$1"
fi
