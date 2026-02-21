-- git-loom completions for clink
-- Setup: save to %LocalAppData%\clink\git-loom.lua
-- Or load dynamically: load(io.popen('git-loom completions clink'):read("*a"))()

local branch_matcher = clink.argmatcher()
    :addflags("-t", "--target", "--help", "-h")

local reword_matcher = clink.argmatcher()
    :addflags("-m", "--message", "--help", "-h")

local commit_matcher = clink.argmatcher()
    :addflags("-b", "--branch", "-m", "--message", "--help", "-h")

clink.argmatcher("git-loom")
    :addarg(
        "status",
        "init",
        "branch"       .. branch_matcher,
        "reword"       .. reword_matcher,
        "commit"       .. commit_matcher,
        "drop",
        "fold",
        "update"
    )
    :addflags("--no-color", "--help", "-h")
