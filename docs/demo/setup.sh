#!/bin/bash
# Usage: setup.sh <cli|tui> — rebuilds $DEMO/work/<mode> with a fake "taskflow" project.
set -e
source "$(dirname "$0")/env.sh"
MODE=$1
W="$DEMO/work/$MODE"
rm -rf "$W"; mkdir -p "$W"; cd "$W"

upstream() { # name email date message
  GIT_AUTHOR_NAME="$1" GIT_AUTHOR_EMAIL="$2" GIT_COMMITTER_NAME="$1" GIT_COMMITTER_EMAIL="$2" \
  GIT_AUTHOR_DATE="$3" GIT_COMMITTER_DATE="$3" git commit -q -m "$4"
}

git init -q --bare origin.git
git init -q seed; cd seed
mkdir -p app static templates

cat > README.md <<'EOF'
# Taskflow

A tiny task tracker.
EOF
cat > app/__init__.py <<'EOF'
from flask import Flask


def create_app():
    app = Flask(__name__)
    from . import routes
    app.register_blueprint(routes.bp)
    return app
EOF
git add -A; upstream "Priya Shah" priya@taskflow.dev "2026-10-05T09:12:00+0200" "Initial project skeleton"

cat > app/models.py <<'EOF'
from dataclasses import dataclass, field
from datetime import datetime


@dataclass
class Task:
    id: int
    title: str
    done: bool = False
    created: datetime = field(default_factory=datetime.now)


TASKS: list[Task] = []
EOF
git add -A; upstream "Marco Bellini" marco@taskflow.dev "2026-10-06T11:40:00+0200" "Add Task model"

cat > app/routes.py <<'EOF'
from flask import Blueprint, jsonify, render_template, request

from .models import TASKS, Task

bp = Blueprint("tasks", __name__)


@bp.get("/")
def index():
    return render_template("index.html", tasks=TASKS)


@bp.post("/tasks")
def add_task():
    task = Task(id=len(TASKS) + 1, title=request.json["title"])
    TASKS.append(task)
    return jsonify(task), 201


@bp.post("/tasks/<int:task_id>/done")
def complete_task(task_id):
    TASKS[task_id - 1].done = True
    return "", 204
EOF
cat > templates/index.html <<'EOF'
<!doctype html>
<html>
<head>
  <title>Taskflow</title>
  <link rel="stylesheet" href="/static/style.css">
</head>
<body>
  <h1>Taskflow</h1>
  <ul>
    {% for task in tasks %}
    <li class="task">{{ task.title }}</li>
    {% endfor %}
  </ul>
</body>
</html>
EOF
git add -A; upstream "Priya Shah" priya@taskflow.dev "2026-10-08T16:05:00+0200" "Add task list page and REST endpoint"

cat > static/style.css <<'EOF'
body {
  font-family: system-ui, sans-serif;
  background: #ffffff;
  color: #1f2328;
}

.task {
  padding: 0.25rem 0;
}
EOF
git add -A; upstream "Lena Koch" lena@taskflow.dev "2026-10-09T10:30:00+0200" "Add base stylesheet"
git push -q "$W/origin.git" HEAD:main
cd ..; rm -rf seed

git clone -q origin.git taskflow; cd taskflow
# Looks like GitHub; fake-ssh.sh serves it from origin.git, bin/gh.exe stubs the gh CLI.
git remote set-url origin git@github.com:acme/taskflow.git
git config core.sshCommand "$DEMO/fake-ssh.sh $W/origin.git"
git config ssh.variant simple
git config loom.remote-type github
rm -f "$FAKE_GH_STATE"
git loom init >/dev/null

# feature-search: two commits, woven in. The search is case-sensitive on purpose (absorbed later).
cat > app/search.py <<'EOF'
from .models import TASKS


def search(query):
    return [t for t in TASKS if query in t.title]
EOF
cat >> app/routes.py <<'EOF'


@bp.get("/search")
def search_tasks():
    return jsonify(search(request.args.get("q", "")))
EOF
sed -i 's/^from .models import TASKS, Task$/from .models import TASKS, Task\nfrom .search import search/' app/routes.py
git loom commit -b feature-search -m "Add search endpoint" zz >/dev/null

sed -i 's|^  <ul>$|  <form action="/search"><input name="q" placeholder="Search tasks"></form>\n  <ul>|' templates/index.html
git loom commit -b feature-search -m "Add search box to task list" zz >/dev/null

if [ "$MODE" = tui ]; then
  "$DEMO/scene.sh" darkmode-edit
  git loom commit -b dark-mode -m "Add dark mode toggle" zz >/dev/null
  "$DEMO/scene.sh" tui-edits
fi
echo "ready: $W/taskflow"
