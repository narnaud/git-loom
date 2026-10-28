# git-loom 1.0

*Weave your branches together.*

git-loom is a Git extension for working on several features at once. You stay on one integration branch that merges all your feature branches, so you build and test them together, and from there loom commits to, rewrites and pushes each branch on its own. You never switch branches, and you never rebase by hand.

It is plain Git all the way down. Feature branches are ordinary branches, the integration branch is ordinary merges, and every other Git tool keeps working on the same repository.

With 1.0, the command line, the `loom.*` settings and agent mode are stable: see [Compatibility](compatibility.md).

## See It in Action

From the command line:

<video src="assets/git-loom-cli.mp4" controls muted playsinline preload="metadata" width="100%"></video>

The same operations from the TUI:

<video src="assets/git-loom-tui.mp4" controls muted playsinline preload="metadata" width="100%"></video>

## The Idea

Your integration branch tracks the upstream, `origin/main` for instance, and merges in each feature branch as a side branch. `git loom status`, or just `git loom`, draws it:

```
╭─ zz [local changes]
│   ma  M src/main.rs
│   nf A  new_file.rs
│   un  ⁕ untracked.txt
│
│╭─ fb [feature-b] ✓
│●    pop  d0472f9 Fix bug in feature B
│●    hug  7a067a9 Start feature B
├╯
│
│╭─ fa [feature-a] ↑
│●    dog  2ee61e1 Add feature A
├╯
│
● ff1b247 (upstream) [origin/main] Initial commit
```

Each branch is pushed and reviewed on its own; the integration branch is where you work on all of them together. Every line carries a short ID (`zz`, `fb`, `pop`, `ma`) that other commands take in place of a branch name, a hash or a path.

## Pick How You Work

Every loom operation is there three ways, and you can mix them in the same repository:

- **The command line.** Each operation is one command on short IDs: `git loom fold ma pop` amends `src/main.rs` into commit `pop`. Completions cover PowerShell, Clink, bash, zsh and fish.
- **The TUI.** [`git loom tui`](commands/tui.md) puts the branch tree beside the diff of the row under the cursor, with loom's actions one key away: `c` commits the selected files, `f` folds them, `s` splits a commit, `p` pushes a branch. When an action needs a target, you move the new commit or the fold through the tree with the arrow keys and place it with `Enter`. A conflict opens a paused view where you stage the resolved files, then continue or abort.
- **An AI agent.** [`git loom agent install`](guides/agents.md) teaches Claude Code to use loom. Ask for the result, "put this CSRF fix into the login form commit", and it runs `git loom fold src/auth.rs pop`. Loom never guesses on its behalf: when a choice is missing, it answers with the question, changes no history, and the agent brings the question to you. Any other agent or script can drive loom with `--agent`, which ends every command with one line of JSON.

## What You Can Do

- **Commit to any branch** without leaving the integration branch: `git loom commit -b feature-a -m "…" <files>`. A new name creates the branch.
- **Rewrite history** with one command each: `fold` amends a past commit, fixes it up, or moves commits and files between commits and branches; `split`, `swap`, `reword` and `drop` do what they say; `absorb` sends each change to the commit that last touched its lines.
- **Pick hunks**: `-p` on `add`, `commit`, `fold` and `split` opens a hunk picker.
- **Stack branches**: `git loom branch --on <branch>` starts a branch on top of another, and `push` sends the whole stack.
- **Push for review**: one pull request per branch on GitHub (stacked PRs, forks), GitLab, Azure DevOps or Gerrit, or a plain push anywhere else.
- **Stay up to date**: `update` pull-rebases the whole weave, updates submodules, and offers to remove branches merged upstream. `update <branch>` takes a branch's remote version, pushed from another machine or by a teammate.
- **Bring branches in and out**: `merge` weaves in an existing branch, a colleague's included; `unmerge` takes it out and keeps the branch; `switch` checks out any branch to test it alone.
- **Recover from anything**: a conflict pauses the operation until you `continue` or `abort`. No loom command ever discards staged, unstaged or uncommitted changes, `abort` included.

## Worktrees: Several Integration Branches at Once

Need a second line of work going at the same time, such as a hotfix while a long build runs, or a second agent working in parallel? Give it a [worktree](commands/worktree.md). Each worktree holds its own integration branch, tracking the same upstream:

```bash
git loom wt new hotfix
# ✓ Created worktree /src/repo-hotfix on integration-hotfix tracking origin/main
cd "$(git loom wt path hotfix)"
```

Every loom command works there as in the main checkout, and a paused operation stays private to its worktree. `git loom wt` lists the worktrees and marks the dirty ones; `git loom wt drop` removes one, and keeps its integration branch if it holds commits found nowhere else. A feature branch is woven into one integration branch at a time, so `merge` refuses a branch already woven into another worktree. A worktree made with `git worktree add` works too, after a `git loom init` inside it.

## What Makes It Different

### Mix and Match with Raw Git

Loom adds commands to Git; it does not take Git away. Use whichever is handier for each step:

- Stage with `git add -p` or your IDE, then `git loom commit -b <branch>` commits exactly what is staged.
- Commit straight to the integration branch with `git commit`: it shows up as a loose commit, and `git loom branch <name> --on <commit>` turns it into a feature branch later.
- Make a branch with plain Git, or fetch a colleague's, and weave it in with `git loom merge`.
- Inspect with `git log`, `git blame` or any Git GUI: the weave is ordinary merge commits.
- `git loom commit` runs your hooks, and anything after `--` goes to the git command behind a loom command: `git loom commit -m "wip" -- --no-verify`.

Rewriting the integration branch is the one thing to leave to loom: a plain `git rebase` would flatten the merges that hold the weave.

### Short IDs You Can Say Out Loud

Every branch, commit and file in the status gets a short ID, and `zz` always means all your local changes. Branches and files take two letters from their name: `fa` for `feature-a`, `ma` for `main.rs`. Commits that loom creates get a persistent ID, such as `pop`, `sun` or `dog`, in which consonants and vowels alternate, so every ID can be said aloud and many are words.

Like jj's change IDs, a persistent ID survives every rewrite, so you can chain commands on it. Unlike them, it comes from a standard `Change-Id` trailer in the commit message, the one Gerrit uses, so it survives any Git tool that keeps the message.

```bash
git loom fold ma pop                    # amend src/main.rs into commit pop
git loom reword pop -m "Fix the parser" # still pop
```

### Local Branches

A branch whose name starts with `local-` is woven like any other, so its commits are in everything you build and run. But `status` hides it, and `push` never publishes it. Keep your personal configuration, debug logging or a workaround you are not ready to share there, and it never leaks into a pull request. `git loom status --all` shows it, and [`loom.hideBranchPattern`](configuration.md#loomhidebranchpattern) changes the prefix.

### Builds That Stay Incremental

A Git rebase rewrites every file the replayed commits touch, even when its content ends up the same, and the fresh timestamp makes `make`, `cargo` or `ninja` rebuild it. Loom records modification times before each rebase and puts them back on every file whose bytes did not change. After a `reword`, `swap` or `branch`, nothing rebuilds; after a `fold` or `split`, only what changed does. Pause watch-mode builds while a loom command runs: they would compile half-rewritten files.

## How It Works: the Weave Engine

Loom does not reimplement rebase. Every history-changing command builds a model of the integration branch (its base, each woven branch and its commits, the merges that join them), changes that model in memory, and writes the complete `git rebase --rebase-merges` todo from it. It then runs one native interactive rebase with itself as the sequence editor, so the todo Git executes is exactly the one loom wrote. Git does the work: `--update-refs` moves every branch along, a conflict stops as in any rebase, and `git loom abort` puts back the history, the branches and every uncommitted change.

## How It Compares

git-loom borrows from the two tools that inspired it: persistent change IDs and `absorb` from jujutsu, working on many branches at once from GitButler. All three work with Git repositories; they differ in how much of Git's own model you keep.

### jujutsu (jj)

jj is a version control system of its own. It can store its commits in a regular Git repository, and a colocated workspace lets you mix `jj` and `git` commands, but its model is different: the working copy is committed automatically, there is no index, and Git usually sees a detached HEAD. git-loom keeps Git's model. You stay on a branch, and `git add`, the index and `git commit` work as always; loom only adds commands on top.

### GitButler

GitButler is the closest in spirit. Its parallel branches are applied together in one working directory, and Git sees them as a merge commit on a `gitbutler/workspace` branch that GitButler rebuilds as you change branches. That branch belongs to GitButler, though: a pre-commit hook refuses `git commit` on it, and switching branches with `git switch` pauses GitButler until you come back through it. git-loom's integration branch is yours: `git commit` works on it, and you can switch away with git and come back, with no mode to leave or set up again (see [Mix and Match with Raw Git](#mix-and-match-with-raw-git)).

## Commands

```
Workflow:
  init              Initialize a new integration branch
  update, up        Pull-rebase and update submodules
  push, pr          Push a branch to remote
  agent             Install the loom skill for AI agents

Staging:
  add               Stage files using short IDs or paths [-p for interactive hunks]

Commits:
  commit, ci        Create a commit on a feature branch [-p for interactive hunks]
  fold              Amend, fixup, or move commits [-p for interactive hunks] [amend, fixup, mv, rub]
  absorb            Auto-distribute changes into originating commits
  split             Split a commit into two [-p for interactive hunks]
  swap              Swap two commits
  reword, rw        Reword a commit message or rename a branch
  drop, rm          Drop a change, commit, or branch

Branches:
  branch, br        Create a feature branch
  merge             Merge an existing branch into integration
  unmerge           Remove a branch from integration (keeps the ref)
  switch, sw        Switch to any branch for testing (without merging)
  worktree, wt      Manage worktrees, each with its own integration branch (list by default, new, drop, path)

Inspection:
  status            Show the branch-aware status (default command)
  tui               Interactive status TUI (tree + diff, with actions)
  show, sh          Show commit details (like git show)
  diff, di          Show a diff using short IDs (like git diff)
  trace             Show the latest command trace

Recovery:
  continue, cont    Resume a paused operation after resolving conflicts
  abort             Cancel a paused operation and restore original state
```

## Configuration

Set these with `git config <setting> <value>`; [Configuration](configuration.md) has the details.

| Setting | Default | What it does |
|---------|---------|--------------|
| `loom.remoteType` | Detected, else asked | How `push` pushes: `github`, `gitlab`, `azure`, `gerrit` or `plain` |
| `loom.pushRemote` | Detected | The remote `push` pushes to |
| `loom.hideBranchPattern` | `local-` | Prefix of the branches `status` hides and `push` never publishes |
| `loom.statusContext` | `1` | Commits shown at and before the base by `status` and `tui` |
| `loom.pruneGoneBranches` | `false` | Let `update` remove merged and gone branches without asking |
| `loom.changeId` | `true` | Add a `Change-Id` trailer to the commits loom creates |

## Get Started

```bash
cargo install git-loom

# Create an integration branch from your main branch
git checkout main
git loom init

# Commit everything to a new feature branch, see the status, push it
git loom commit -b feature-auth -m "Add the login form" zz
git loom
git loom push feature-auth
```

git-loom needs Git 2.43 or later, and the `gh` or `az` CLI to open pull requests on GitHub or Azure DevOps. It is also on [Scoop and as pre-built binaries](installation.md) for Linux, macOS and Windows.

- [Tutorial](guides/tutorial.md)
- [Working with AI agents](guides/agents.md)
- [Source and releases](https://github.com/narnaud/git-loom)
