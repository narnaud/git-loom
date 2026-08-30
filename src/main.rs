mod absorb;
mod add;
mod agent;
mod branch;
mod commit;
mod completions;
mod core;
mod diff;
mod drop;
mod fold;
mod git;
mod init;
mod push;
mod reword;
mod show;
mod split;
mod status;
mod swap;
mod switch;
mod trace;
mod tui;
mod update;

use crate::agent::AgentKind;
use crate::core::{agent_mode, graph, msg, repo, transaction};

use std::ffi::OsString;
use std::io::IsTerminal;
use std::sync::OnceLock;

use anyhow::Context;
use clap::builder::styling::{AnsiColor, Styles};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use colored::control;

#[derive(ValueEnum, Clone, Copy)]
enum ThemeArg {
    /// Detect from terminal background color (default to dark if undetectable)
    Auto,
    /// Dark terminal background
    Dark,
    /// Light terminal background
    Light,
}

/// Terminal background the colors are tuned for.
#[derive(Clone, Copy, PartialEq)]
enum ThemeMode {
    Dark,
    Light,
}

// Help templates. `{h}`, `{l}` and `{p}` are the header, literal and
// placeholder styles and `{r}` resets; `apply_styles` substitutes them so the
// help output follows the terminal background and `--no-color`.
const ABOUT: &str = "\
{p}Weave your branches together{r}
Checkout the full docs here: https://narnaud.github.io/git-loom/";

const HELP_TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}{after-help}\n\n{h}Options:{r}\n{options}\n";

// Grouped command help, replacing clap's flat command list.
const GROUPED_COMMANDS: &str = "\
{h}Workflow:{r}
  {l}init{r}              Initialize a new integration branch
  {l}update{r}, {l}up{r}        Pull-rebase and update submodules
  {l}push{r}, {l}pr{r}          Push a branch to remote
  {l}agent{r}             Install the loom skill for AI agents

{h}Staging:{r}
  {l}add{r}               Stage files using short IDs or paths [{l}-p{r} for interactive hunks]

{h}Commits:{r}
  {l}commit{r}, {l}ci{r}        Create a commit on a feature branch [{l}-p{r} for interactive hunks]
  {l}fold{r}              Amend, fixup, or move commits [{l}-p{r} for interactive hunks] [{l}amend{r}, {l}am{r}, {l}fixup{r}, {l}mv{r}, {l}rub{r}]
  {l}absorb{r}            Auto-distribute changes into originating commits
  {l}split{r}             Split a commit into two [{l}-p{r} for interactive hunks]
  {l}swap{r}              Swap two commits
  {l}reword{r}, {l}rw{r}        Reword a commit message or rename a branch
  {l}drop{r}, {l}rm{r}          Drop a change, commit, or branch

{h}Branches:{r}
  {l}branch{r}, {l}br{r}        Manage feature branches (create, merge, unmerge)
  {l}switch{r}, {l}sw{r}        Switch to any branch for testing (without weaving)

{h}Inspection:{r}
  {l}status{r}            Show the branch-aware status ({p}default{r} command)
  {l}show{r}, {l}sh{r}          Show commit details (like git show)
  {l}diff{r}, {l}di{r}          Show a diff using short IDs (like git diff)
  {l}trace{r}             Show the latest command trace

{h}Recovery:{r}
  {l}continue{r}, {l}c{r}       Resume a paused operation after resolving conflicts
  {l}abort{r}, {l}a{r}          Cancel a paused operation and restore original state";

/// Style palette for the help output. Yellow is unreadable on a light
/// background, so headers move to blue and placeholders to magenta there.
/// Returns a plain palette (no escapes at all) when colors are disabled.
fn help_styles(mode: ThemeMode, color: bool) -> Styles {
    if !color {
        return Styles::plain();
    }
    let (header, literal, placeholder) = match mode {
        ThemeMode::Dark => (AnsiColor::Yellow, AnsiColor::Green, AnsiColor::Blue),
        ThemeMode::Light => (AnsiColor::Blue, AnsiColor::Green, AnsiColor::Magenta),
    };
    Styles::styled()
        .header(header.on_default().bold())
        .usage(header.on_default().bold())
        .literal(literal.on_default())
        .placeholder(placeholder.on_default())
}

/// Substitute the style markers of a help template.
fn apply_styles(template: &str, styles: &Styles) -> String {
    let header = styles.get_header();
    template
        .replace("{h}", &header.to_string())
        .replace("{l}", &styles.get_literal().to_string())
        .replace("{p}", &styles.get_placeholder().to_string())
        .replace("{r}", &format!("{header:#}"))
}

#[derive(Parser)]
#[command(name = "git-loom", version)]
struct Cli {
    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Machine-readable JSON status output for AI agents (see also LOOM_AGENT)
    #[arg(long, global = true)]
    agent: bool,

    /// Color theme for graph output
    #[arg(long, default_value = "auto")]
    theme: ThemeArg,

    /// Show files changed in each commit (optionally filtered to specific commits)
    #[arg(short = 'f', long = "files", num_args = 0.., hide = true)]
    files: Option<Vec<String>>,

    /// Number of context commits to show before the base
    #[arg(default_value = "1", hide = true)]
    context: usize,

    /// Show all branches including hidden ones (those matching loom.hideBranchPattern)
    #[arg(short = 'a', long = "all", hide = true)]
    all: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    // -- Workflow --
    /// Initialize a new integration branch tracking a remote
    Init {
        /// Branch name (defaults to "integration")
        name: Option<String>,
    },
    /// Pull-rebase the integration branch and update submodules
    #[command(visible_alias = "up")]
    Update {
        /// Remove local branches whose upstream tracking branch was deleted on remote
        #[arg(short, long)]
        yes: bool,
    },
    /// Push a feature branch to remote and optionally create a PR or Gerrit review
    #[command(visible_alias = "pr")]
    Push {
        /// Branch name or short ID (if not provided, will prompt interactively)
        branch: Option<String>,
        /// Push branch without creating a PR or Gerrit review
        #[arg(long)]
        no_pr: bool,
    },
    /// Set up AI agent integration (unrelated to `init`)
    Agent(AgentCmd),

    // -- Staging --
    /// Stage files using short IDs, paths, or 'zz' for all
    Add {
        /// Files to stage (short IDs, filenames, or 'zz' for all)
        #[arg(num_args = 0..)]
        files: Vec<String>,
        /// Interactively select hunks to stage
        #[arg(short = 'p', long = "patch")]
        patch: bool,
    },

    // -- Commits --
    /// Create a commit on a feature branch without leaving integration
    #[command(visible_alias = "ci")]
    Commit {
        /// Target feature branch (name or short ID)
        #[arg(short = 'b', long = "branch")]
        branch: Option<String>,
        /// Commit message (if not provided, opens editor)
        #[arg(short, long)]
        message: Option<String>,
        /// Interactively select hunks to stage before committing
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        /// Files to stage (short IDs, filenames, or 'zz' for all), none for all tracked changes
        files: Vec<String>,
    },
    /// Fold source(s) into a target (amend files, fixup commits, move commits, move files between commits)
    #[command(visible_aliases = ["amend", "am", "fixup", "mv", "rub"])]
    Fold {
        /// Create a new branch from the source commit(s) and move them there
        #[arg(short = 'c', long = "create")]
        create: bool,
        /// Interactively select hunks to stage before folding
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        /// Source(s) and target: files, commits, or branches (last arg is the target)
        #[arg(required = true, num_args = 1..)]
        args: Vec<String>,
    },
    /// Absorb working tree changes into the commits that introduced them
    Absorb {
        /// Show what would be absorbed without making changes
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Files to restrict absorption to (default: all tracked changed files)
        files: Vec<String>,
    },
    /// Split a commit into two sequential commits
    Split {
        /// Commit hash, short ID, or HEAD
        target: String,
        /// Message for the first commit (prompts if omitted)
        #[arg(short, long)]
        message: Option<String>,
        /// Interactively pick hunks for the first commit
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        /// Files for the first commit (shows interactive picker if omitted)
        files: Vec<String>,
    },
    /// Reword a commit message or rename a branch
    #[command(visible_alias = "rw")]
    Reword {
        /// Branch name, shortID, or commit hash
        target: String,
        /// New message or branch name (if not provided, opens editor for commits)
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Swap two commits within the same sequence
    Swap {
        /// First commit hash or short ID
        a: String,
        /// Second commit hash or short ID
        b: String,
    },
    /// Drop a local change, a commit, or a branch from history
    #[command(visible_alias = "rm")]
    Drop {
        /// Commit hash, branch name, or short ID to drop
        target: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    // -- Branches --
    /// Manage feature branches (create, merge, unmerge)
    #[command(visible_alias = "br")]
    Branch(BranchCmd),

    /// Switch to any branch for testing without weaving it into the integration branch
    #[command(visible_alias = "sw")]
    Switch {
        /// Branch name or short ID (if not provided, shows interactive picker)
        branch: Option<String>,
    },

    // -- Inspection --
    /// Show the branch-aware status
    Status {
        /// Show files changed in each commit (optionally filtered to specific commits)
        #[arg(short = 'f', long = "files", num_args = 0.., value_name = "COMMIT")]
        files: Option<Vec<String>>,
        /// Number of context commits to show before the base
        #[arg(default_value = "1")]
        context: usize,
        /// Show all branches including hidden ones (those matching loom.hideBranchPattern)
        #[arg(short = 'a', long = "all")]
        all: bool,
    },
    /// Show the diff and metadata for a commit (like `git show`)
    #[command(visible_alias = "sh")]
    Show {
        /// Commit hash, branch name, or short ID (defaults to the last commit on the current
        /// branch). Options loom doesn't know are passed to `git show`
        #[arg(num_args = 0.., allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show a diff using short IDs (like `git diff`)
    #[command(visible_alias = "di")]
    Diff {
        /// Files, commits, or commit ranges (short IDs supported, e.g. `ma`, `d0`, `d0..3a`).
        /// Options loom doesn't know are passed to `git diff`
        #[arg(num_args = 0.., allow_hyphen_values = true)]
        args: Vec<String>,
        /// Show staged changes (index vs HEAD)
        #[arg(long = "staged", visible_alias = "cached", conflicts_with = "all")]
        staged: bool,
        /// Show all changes, both staged and unstaged (working tree vs HEAD)
        #[arg(short = 'a', long = "all")]
        all: bool,
    },
    /// Show the latest command trace
    Trace,

    // -- Recovery --
    /// Resume a paused loom operation after resolving conflicts
    #[command(visible_alias = "c")]
    Continue,
    /// Cancel a paused loom operation and restore original state
    #[command(visible_alias = "a")]
    Abort,

    // -- Hidden --
    /// Generate shell completions (powershell, clink)
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for (powershell, clink)
        shell: String,
    },
    /// Internal: used as GIT_SEQUENCE_EDITOR to write a pre-generated todo file
    #[command(hide = true)]
    InternalWriteTodo {
        /// Path to the source file containing the todo content
        #[arg(long = "source")]
        source: String,
        /// Path to the git rebase todo file (provided by git)
        todo_file: String,
    },
}

#[derive(Args)]
struct AgentCmd {
    #[command(subcommand)]
    action: AgentAction,
}

#[derive(Subcommand)]
enum AgentAction {
    /// Install the loom skill for an AI agent
    Init {
        /// AI agent to install the skill for
        // Named `kind`, not `agent`: the global `--agent` flag already owns
        // that clap id and the two would collide inside this subcommand.
        #[arg(value_enum, default_value = "claude", value_name = "AGENT")]
        kind: AgentKind,

        /// Install into the repository (e.g. .claude/skills/) instead of the home directory
        #[arg(long, conflicts_with = "dir")]
        project: bool,

        /// Override the install base directory (the skills/git-loom/SKILL.md suffix is appended)
        #[arg(long, hide = true)]
        dir: Option<std::path::PathBuf>,
    },
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct BranchCmd {
    #[command(subcommand)]
    action: Option<BranchAction>,

    #[command(flatten)]
    new_args: BranchNewArgs,
}

#[derive(Subcommand)]
enum BranchAction {
    /// Create a new feature branch
    #[command(visible_alias = "create")]
    New(BranchNewArgs),

    /// Weave an existing branch into the integration branch
    Merge {
        /// Branch name (if not provided, shows interactive picker)
        branch: Option<String>,

        /// Also show remote branches without a local counterpart
        #[arg(short = 'a', long = "all")]
        all: bool,
    },

    /// Remove a branch from the integration branch (keeps the branch ref)
    Unmerge {
        /// Branch name or short ID (if not provided, shows interactive picker)
        branch: Option<String>,
    },
}

#[derive(Args, Clone)]
struct BranchNewArgs {
    /// Branch name (if not provided, will prompt interactively)
    name: Option<String>,

    /// Target commit, branch, or shortID (defaults to upstream base)
    #[arg(short = 't', long = "target")]
    target: Option<String>,
}

/// Message shown when a command is blocked by a paused loom operation.
///
/// `interrupted` tells whether git still has a rebase or merge going: if not,
/// the user likely finished it by hand, and `loom continue` only has the
/// post-rebase bookkeeping left to do.
fn paused_state_message(command: &str, interrupted: bool) -> String {
    if interrupted {
        format!(
            "A `loom {command}` is paused due to conflicts.\n\
             Resolve them, then run `loom continue` to resume, or `loom abort` to cancel."
        )
    } else {
        format!(
            "A `loom {command}` is paused, but no rebase is in progress.\n\
             If you finished it yourself, run `loom continue` to wrap up and clear the state.\n\
             Run `loom abort` to discard it instead."
        )
    }
}

fn main() {
    let cli = parse_cli(&std::env::args_os().collect::<Vec<_>>());

    // Explicit opt-in only — never inferred from a missing terminal. The
    // sequence-editor subprocess is excluded: it inherits LOOM_AGENT from its
    // parent loom process, but its stderr flows through git and must stay clean.
    let is_subprocess = matches!(cli.command, Some(Command::InternalWriteTodo { .. }));
    agent_mode::set(
        !is_subprocess
            && (cli.agent
                || std::env::var_os("LOOM_AGENT").is_some_and(|v| !v.is_empty() && v != "0")),
    );

    if !colors_enabled(cli.no_color) {
        control::set_override(false);
    }

    // Completions don't need git, handle before version check
    if let Some(Command::Completions { shell }) = cli.command {
        if let Err(e) = completions::run(shell) {
            msg::error(&e.to_string());
            std::process::exit(1);
        }
        return;
    }

    // Agent setup needs no git or repo either (unless --project), works while
    // an operation is paused, and is never trace-logged.
    if let Some(Command::Agent(cmd)) = cli.command {
        let result = match cmd.action {
            AgentAction::Init { kind, project, dir } => agent::init::run(kind, project, dir),
        };
        finish_and_exit(result);
    }

    if let Err(e) = git::check_git_version() {
        msg::error(&e.to_string());
        std::process::exit(1);
    }

    // Initialize logger for commands that modify the repo (skip for
    // InternalWriteTodo — it runs as a subprocess — and Status/Trace/Show which are read-only).
    let should_log = !matches!(
        cli.command,
        Some(Command::InternalWriteTodo { .. })
            | Some(Command::Trace)
            | Some(Command::Show { .. })
            | Some(Command::Diff { .. })
    );
    if should_log && let Ok(repo) = repo::open_repo() {
        let git_dir = repo.path().to_path_buf();
        let cmd_line = std::env::args().collect::<Vec<_>>().join(" ");
        if matches!(cli.command, Some(Command::Abort) | Some(Command::Continue)) {
            trace::init_appending(&git_dir, &cmd_line);
        } else {
            trace::init(&git_dir, &cmd_line);
        }
    }

    // Check for a paused loom operation and block most commands if one exists.
    // Exempt: show, trace, continue, abort, completions, internal-write-todo.
    let is_exempt = matches!(
        cli.command,
        Some(Command::Show { .. })
            | Some(Command::Diff { .. })
            | Some(Command::Trace)
            | Some(Command::Continue)
            | Some(Command::Abort)
            | Some(Command::Completions { .. })
            | Some(Command::InternalWriteTodo { .. })
    );
    if !is_exempt && let Ok(repo) = repo::open_repo() {
        let git_dir = repo.path().to_path_buf();
        if let Ok(Some(state)) = transaction::load(&git_dir) {
            let interrupted =
                git::rebase_is_in_progress(&git_dir) || git::merge_is_in_progress(&git_dir);
            finish_and_exit(Err(anyhow::anyhow!(paused_state_message(
                &state.command,
                interrupted
            ))));
        }
    }

    // The hunk pickers are full-screen terminal UIs — reject `-p` in agent
    // mode before any command stages anything (a guard at the picker itself
    // backstops future call paths).
    if agent_mode::enabled() && uses_patch_flag(&cli.command) {
        finish_and_exit(Err(anyhow::anyhow!(
            "--patch is interactive and unavailable in agent mode\n\
             Pass explicit files instead"
        )));
    }

    let theme = graph_theme(resolve_theme_mode(cli.theme));

    let result = match cli.command {
        None => status::run(cli.files, cli.context, cli.all, theme),
        Some(Command::Status {
            files,
            context,
            all,
        }) => status::run(files, context, all, theme),
        Some(Command::Init { name }) => init::run(name),
        Some(Command::Add { files, patch }) => add::run(files, patch, &theme),
        Some(Command::Switch { branch }) => switch::run(branch),
        Some(Command::Branch(cmd)) => match cmd.action {
            Some(BranchAction::New(args)) => branch::new::run(args.name, args.target),
            Some(BranchAction::Merge { branch, all }) => branch::merge::run(branch, all),
            Some(BranchAction::Unmerge { branch }) => branch::unmerge::run(branch),
            None => branch::new::run(cmd.new_args.name, cmd.new_args.target),
        },
        Some(Command::Reword { target, message }) => reword::run(target, message),
        Some(Command::Commit {
            branch,
            message,
            patch,
            files,
        }) => commit::run(branch, message, patch, files, &theme),
        Some(Command::Swap { a, b }) => swap::run(a, b),
        Some(Command::Drop { target, yes }) => drop::run(target, yes),
        Some(Command::Absorb { dry_run, files }) => absorb::run(dry_run, files),
        Some(Command::Show { args }) => show::run(args),
        Some(Command::Diff { args, staged, all }) => diff::run(args, staged, all),
        Some(Command::Split {
            target,
            message,
            patch,
            files,
        }) => split::run(target, message, patch, files, &theme),
        Some(Command::Push { branch, no_pr }) => push::run(branch, no_pr),
        Some(Command::Update { yes }) => update::run(yes),
        Some(Command::Fold {
            create,
            patch,
            args,
        }) => fold::run(create, patch, args, &theme),
        Some(Command::Trace) => trace::run(),
        Some(Command::Continue) => transaction::continue_run(),
        Some(Command::Abort) => transaction::abort_run(),
        Some(Command::Completions { .. }) | Some(Command::Agent(_)) => unreachable!(),
        Some(Command::InternalWriteTodo { source, todo_file }) => {
            handle_write_todo(&source, &todo_file)
        }
    };

    trace::finalize();

    finish_and_exit(result);
}

/// Whether the command was invoked with `-p`/`--patch`.
fn uses_patch_flag(command: &Option<Command>) -> bool {
    matches!(
        command,
        Some(Command::Add { patch: true, .. })
            | Some(Command::Commit { patch: true, .. })
            | Some(Command::Fold { patch: true, .. })
            | Some(Command::Split { patch: true, .. })
    )
}

/// Report the command result and exit.
///
/// In agent mode the last line of stderr is always a single JSON status (see
/// spec 019); prompts answered structurally (the `NeedsInput` marker) skip the
/// human error line because the JSON is the message.
fn finish_and_exit(result: anyhow::Result<()>) -> ! {
    if agent_mode::enabled() {
        if let Err(e) = &result
            && e.downcast_ref::<agent_mode::NeedsInput>().is_none()
        {
            msg::error(&e.to_string());
        }
        std::process::exit(agent_mode::finish(&result));
    }
    if let Err(e) = result {
        msg::error(&e.to_string());
        std::process::exit(1);
    }
    std::process::exit(0);
}

/// Build the CLI and parse `args`. The help text is generated here rather than
/// in the derive attributes because clap renders it while parsing, before
/// `main` gets a chance to look at `--no-color` or the terminal background.
fn parse_cli(args: &[OsString]) -> Cli {
    let color = colors_enabled(args.iter().any(|arg| arg == "--no-color"));
    let styles = help_styles(resolve_theme_mode(early_theme(args)), color);
    let mut command = Cli::command()
        .styles(styles.clone())
        .about(apply_styles(ABOUT, &styles))
        .after_help(apply_styles(GROUPED_COMMANDS, &styles))
        .help_template(apply_styles(HELP_TEMPLATE, &styles));
    if !color {
        command = command.color(clap::ColorChoice::Never);
    }
    let matches = command.get_matches_from(args);
    match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(err) => err.exit(),
    }
}

/// Whether colored output should be emitted at all.
fn colors_enabled(no_color: bool) -> bool {
    !no_color
        && std::env::var_os("NO_COLOR").is_none()
        && !std::env::var_os("TERM").is_some_and(|v| v == "dumb")
        && std::io::stdout().is_terminal()
}

/// Read `--theme` from the raw args, for the same reason as `parse_cli`.
/// Unknown values fall back to `Auto`; clap reports them during the real parse.
fn early_theme(args: &[OsString]) -> ThemeArg {
    let mut rest = args.iter().skip(1).filter_map(|arg| arg.to_str());
    while let Some(arg) = rest.next() {
        let value = match arg.strip_prefix("--theme") {
            Some("") => rest.next(),
            Some(tail) => tail.strip_prefix("="),
            None => continue,
        };
        if let Some(value) = value
            && let Ok(theme) = ThemeArg::from_str(value, true)
        {
            return theme;
        }
    }
    ThemeArg::Auto
}

fn resolve_theme_mode(arg: ThemeArg) -> ThemeMode {
    match arg {
        ThemeArg::Dark => ThemeMode::Dark,
        ThemeArg::Light => ThemeMode::Light,
        ThemeArg::Auto => detect_theme_mode(),
    }
}

/// Query the terminal for its background color, at most once per run.
fn detect_theme_mode() -> ThemeMode {
    static DETECTED: OnceLock<ThemeMode> = OnceLock::new();
    *DETECTED.get_or_init(|| {
        if !std::io::stdout().is_terminal() {
            return ThemeMode::Dark;
        }
        use terminal_colorsaurus::{QueryOptions, ThemeMode as Detected, theme_mode};
        match theme_mode(QueryOptions::default()) {
            Ok(Detected::Light) => ThemeMode::Light,
            _ => ThemeMode::Dark,
        }
    })
}

fn graph_theme(mode: ThemeMode) -> graph::Theme {
    match mode {
        ThemeMode::Dark => graph::Theme::dark(),
        ThemeMode::Light => graph::Theme::light(),
    }
}

fn handle_write_todo(source: &str, todo_file: &str) -> anyhow::Result<()> {
    // Save the original git todo to a sidecar file (for logging)
    if let Ok(original) = std::fs::read_to_string(todo_file) {
        let sidecar = format!("{}.original", source);
        let _ = std::fs::write(sidecar, original);
    }

    let content = std::fs::read_to_string(source)
        .with_context(|| format!("Failed to read source file '{}'", source))?;
    std::fs::write(todo_file, content)
        .with_context(|| format!("Failed to write todo file '{}'", todo_file))?;
    Ok(())
}

#[cfg(test)]
#[path = "main_test.rs"]
mod tests;
