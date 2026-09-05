-- git-loom completions for clink
-- Setup: save to %LocalAppData%\clink\git-loom.lua
-- Or load dynamically: load(io.popen('git-loom completions clink'):read("*a"))()

-- Flag sets, one per command. Aliases that are a distinct word (fixup, rm, ...)
-- reuse the matcher of the command they stand for; the ones that merely
-- abbreviate it (ci, sh, ...) still work but are left out of the list below.

-- Commands whose only flag is --help.
local plain_matcher = clink.argmatcher()
    :addflags("--help", "-h")

local update_matcher = clink.argmatcher()
    :addflags("-y", "--yes", "--help", "-h")

local push_matcher = clink.argmatcher()
    :addflags("--no-pr", "-f", "--force", "--help", "-h")

local add_matcher = clink.argmatcher()
    :addflags("-p", "--patch", "--help", "-h")

local commit_matcher = clink.argmatcher()
    :addflags("-b", "--branch", "-m", "--message", "-p", "--patch", "--help", "-h")

local fold_matcher = clink.argmatcher()
    :addflags("-c", "--create", "-p", "--patch", "--help", "-h")

local absorb_matcher = clink.argmatcher()
    :addflags("-n", "--dry-run", "--help", "-h")

local split_matcher = clink.argmatcher()
    :addflags("-m", "--message", "-p", "--patch", "--help", "-h")

local reword_matcher = clink.argmatcher()
    :addflags("-m", "--message", "--help", "-h")

local drop_matcher = clink.argmatcher()
    :addflags("-y", "--yes", "--help", "-h")

local status_matcher = clink.argmatcher()
    :addflags("-f", "--files", "-a", "--all", "--help", "-h")

-- `show` has no flags of its own and `diff` only these: every other option is
-- forwarded to git, which clink cannot enumerate.
local show_matcher = plain_matcher

local diff_matcher = clink.argmatcher()
    :addflags("--staged", "--cached", "-a", "--all", "--help", "-h")

local branch_new_matcher = clink.argmatcher()
    :addflags("-t", "--target", "--help", "-h")

local branch_merge_matcher = clink.argmatcher()
    :addflags("-a", "--all", "--help", "-h")

local branch_unmerge_matcher = clink.argmatcher()
    :addflags("--help", "-h")

local branch_matcher = clink.argmatcher()
    :addarg(
        "new"      .. branch_new_matcher,
        "create"   .. branch_new_matcher,
        "merge"    .. branch_merge_matcher,
        "unmerge"  .. branch_unmerge_matcher
    )
    :addflags("-t", "--target", "--help", "-h")

local agent_init_matcher = clink.argmatcher()
    :addarg("claude")
    :addflags("--project", "--agent", "--help", "-h")

local agent_matcher = clink.argmatcher()
    :addarg(
        "init" .. agent_init_matcher
    )
    :addflags("--help", "-h")

local theme_matcher = clink.argmatcher()
    :addarg("auto", "dark", "light")

clink.argmatcher("git-loom")
    :addarg(
        -- Workflow
        "init"      .. plain_matcher,
        "update"    .. update_matcher,
        "push"      .. push_matcher,
        "pr"        .. push_matcher,
        "agent"     .. agent_matcher,
        -- Staging
        "add"       .. add_matcher,
        -- Commits
        "commit"    .. commit_matcher,
        "fold"      .. fold_matcher,
        "amend"     .. fold_matcher,
        "fixup"     .. fold_matcher,
        "mv"        .. fold_matcher,
        "rub"       .. fold_matcher,
        "absorb"    .. absorb_matcher,
        "split"     .. split_matcher,
        "swap"      .. plain_matcher,
        "reword"    .. reword_matcher,
        "drop"      .. drop_matcher,
        "rm"        .. drop_matcher,
        -- Branches
        "branch"    .. branch_matcher,
        "switch"    .. plain_matcher,
        -- Inspection
        "status"    .. status_matcher,
        "tui"       .. plain_matcher,
        "show"      .. show_matcher,
        "diff"      .. diff_matcher,
        "trace"     .. plain_matcher,
        -- Recovery
        "continue"  .. plain_matcher,
        "abort"     .. plain_matcher
    )
    :addflags("--no-color", "--theme" .. theme_matcher, "--agent", "--version", "--help", "-h")
