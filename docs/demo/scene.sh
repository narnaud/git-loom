#!/bin/bash
# Hidden "the developer edited files" steps, run from inside the taskflow clone.
set -e
case "$1" in
darkmode-edit)
  cat >> static/style.css <<'EOF'

body.dark {
  background: #0d1117;
  color: #e6edf3;
}
EOF
  cat > static/theme.js <<'EOF'
const toggle = document.querySelector("#theme-toggle");
const saved = localStorage.getItem("thme");

if (saved === "dark") document.body.classList.add("dark");

toggle.addEventListener("click", () => {
  const dark = document.body.classList.toggle("dark");
  localStorage.setItem("theme", dark ? "dark" : "light");
});
EOF
  ;;
fixes)
  # A fix for each branch: absorb sends each hunk to the commit that wrote the line.
  sed -i 's/if query in t.title\]/if query.lower() in t.title.lower()]/' app/search.py
  sed -i 's/getItem("thme")/getItem("theme")/' static/theme.js
  ;;
hunks)
  # routes.py gets two unrelated hunks: title validation (new branch) and a search tweak (absorbed).
  sed -i 's/^    task = Task(id=len(TASKS) + 1, title=request.json\["title"\])$/    title = request.json["title"].strip()\n    if not title:\n        return jsonify(error="title is required"), 400\n    task = Task(id=len(TASKS) + 1, title=title)/' app/routes.py
  sed -i 's/search(request.args.get("q", ""))/search(request.args.get("q", "").strip())/' app/routes.py
  sed -i 's/if query in t.title\]/if query.lower() in t.title.lower()]/' app/search.py
  sed -i 's/getItem("thme")/getItem("theme")/' static/theme.js
  ;;
amend-css)
  sed -i 's/background: #0d1117;/background: #161b22;/' static/style.css
  ;;
toggle-button)
  sed -i 's|^  <link rel="stylesheet" href="/static/style.css">$|&\n  <script src="/static/theme.js" defer></script>|' templates/index.html
  ;;
teammate)
  # Someone else lands work on origin/main.
  d=$(mktemp -d); git clone -q "$(git config core.sshCommand | cut -d' ' -f2)" "$d"
  ( cd "$d"
    sed -i 's/^    done: bool = False$/    done: bool = False\n    due: datetime | None = None/' app/models.py
    GIT_AUTHOR_NAME="Marco Bellini" GIT_AUTHOR_EMAIL=marco@taskflow.dev \
    GIT_COMMITTER_NAME="Marco Bellini" GIT_COMMITTER_EMAIL=marco@taskflow.dev \
    GIT_AUTHOR_DATE="2026-10-12T15:20:00+0200" GIT_COMMITTER_DATE="2026-10-12T15:20:00+0200" \
      git commit -qam "Add task due dates"
    git push -q origin HEAD:main )
  rm -rf "$d"
  ;;
theme-log)
  sed -i 's/^  const dark = document.body.classList.toggle("dark");$/&\n  console.log("theme", dark);/' static/theme.js
  ;;
tui-edits)
  # search.py is amended into its commit; routes.py has two hunks, one picked for a new branch.
  sed -i 's/if query in t.title\]/if query.lower() in t.title.lower()]/' app/search.py
  sed -i 's/^    task = Task(id=len(TASKS) + 1, title=request.json\["title"\])$/    title = request.json["title"].strip()\n    if not title:\n        return jsonify(error="title is required"), 400\n    task = Task(id=len(TASKS) + 1, title=title)/' app/routes.py
  sed -i 's/search(request.args.get("q", ""))/search(request.args.get("q", "").strip())/' app/routes.py
  ;;
esac
