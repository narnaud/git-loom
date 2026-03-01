-- git-loom completions for clink
-- Setup: save to %LocalAppData%\clink\git-loom.lua
-- Or load dynamically: load(io.popen('git-loom completions clink'):read("*a"))()

local status_matcher = clink.argmatcher()
    :addflags("-f", "--files", "--help", "-h")

local branch_matcher = clink.argmatcher()
    :addflags("-t", "--target", "--help", "-h")

local reword_matcher = clink.argmatcher()
    :addflags("-m", "--message", "--help", "-h")

local commit_matcher = clink.argmatcher()
    :addflags("-b", "--branch", "-m", "--message", "--help", "-h")

local drop_matcher = clink.argmatcher()
    :addflags("-y", "--yes", "--help", "-h")

local split_matcher = clink.argmatcher()
    :addflags("-m", "--message", "--help", "-h")

local absorb_matcher = clink.argmatcher()
    :addflags("-n", "--dry-run", "--help", "-h")

clink.argmatcher("git-loom")
    :addarg(
        "status"       .. status_matcher,
        "init",
        "branch"       .. branch_matcher,
        "reword"       .. reword_matcher,
        "commit"       .. commit_matcher,
        "drop"         .. drop_matcher,
        "fold",
        "split"        .. split_matcher,
        "absorb"       .. absorb_matcher,
        "update",
        "push"
    )
    :addflags("--no-color", "-f", "--files", "--help", "-h")
