#!/bin/sh
# core.sshCommand for the demo: serves git@github.com:acme/taskflow.git from a local bare repo ($1).
origin=$1; shift
for last; do :; done
case "$last" in
  git-upload-pack*) exec git upload-pack "$origin" ;;
  git-receive-pack*) exec git receive-pack "$origin" ;;
  *) echo "fake-ssh: unsupported: $last" >&2; exit 1 ;;
esac
