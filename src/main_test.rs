use super::*;

/// Every marker must be substituted: a leftover `{h}`/`{l}`/`{p}`/`{r}` would
/// be printed verbatim in the help output.
fn assert_no_markers(rendered: &str) {
    for marker in ["{h}", "{l}", "{p}", "{r}"] {
        assert!(
            !rendered.contains(marker),
            "marker {marker} left in: {rendered}"
        );
    }
}

#[test]
fn styles_are_substituted_in_help_templates() {
    let styles = help_styles(ThemeMode::Dark, true);
    for template in [ABOUT, HELP_TEMPLATE, GROUPED_COMMANDS] {
        let rendered = apply_styles(template, &styles);
        assert_no_markers(&rendered);
        assert!(rendered.contains('\u{1b}'), "expected colors: {rendered}");
    }
}

#[test]
fn no_color_leaves_no_escape_sequence() {
    for template in [ABOUT, HELP_TEMPLATE, GROUPED_COMMANDS] {
        let rendered = apply_styles(template, &help_styles(ThemeMode::Dark, false));
        assert_no_markers(&rendered);
        assert!(
            !rendered.contains('\u{1b}'),
            "expected plain text: {rendered}"
        );
    }
}

#[test]
fn light_and_dark_help_use_different_colors() {
    let dark = apply_styles(GROUPED_COMMANDS, &help_styles(ThemeMode::Dark, true));
    let light = apply_styles(GROUPED_COMMANDS, &help_styles(ThemeMode::Light, true));
    assert_ne!(dark, light);
    // Yellow (SGR 33) is unreadable on a light background.
    assert!(dark.contains("\u{1b}[33m"));
    assert!(!light.contains("\u{1b}[33m"));
}

#[test]
fn help_text_layout_is_preserved() {
    let plain = apply_styles(GROUPED_COMMANDS, &help_styles(ThemeMode::Dark, false));
    assert!(plain.starts_with("Workflow:\n  init              Initialize"));
    assert!(plain.contains("\n\nCommits:\n"));
    assert!(plain.contains("  status            Show the branch-aware status (default command)\n"));
}

#[test]
fn early_theme_reads_the_raw_args() {
    let args = |extra: &[&str]| -> Vec<OsString> {
        std::iter::once("git-loom")
            .chain(extra.iter().copied())
            .map(OsString::from)
            .collect()
    };

    assert!(matches!(early_theme(&args(&[])), ThemeArg::Auto));
    assert!(matches!(
        early_theme(&args(&["--theme", "light"])),
        ThemeArg::Light
    ));
    assert!(matches!(
        early_theme(&args(&["--theme=dark", "status"])),
        ThemeArg::Dark
    ));
    assert!(matches!(
        early_theme(&args(&["--theme=LIGHT"])),
        ThemeArg::Light
    ));
    // Invalid or incomplete values are left for clap to report.
    assert!(matches!(
        early_theme(&args(&["--theme=nope"])),
        ThemeArg::Auto
    ));
    assert!(matches!(early_theme(&args(&["--theme"])), ThemeArg::Auto));
}

/// clap validates arg ids, conflicts, and groups only when the command is
/// built; this catches a typo in e.g. `conflicts_with` that would otherwise
/// panic at runtime.
#[test]
fn cli_definition_is_valid() {
    Cli::command().debug_assert();
}

/// The completion scripts copy clap's command and flag tables by hand, and had
/// already drifted. Only checks that each name appears somewhere in a script,
/// not under the right subcommand.
#[test]
fn completion_scripts_cover_the_cli() {
    fn walk(cmd: &clap::Command, path: &str, out: &mut Vec<(String, String)>) {
        for arg in cmd
            .get_arguments()
            // Agents pass `--agent` themselves; nobody tab-completes it.
            .filter(|a| !a.is_hide_set() && a.get_id() != "agent")
        {
            for flag in arg
                .get_long()
                .map(|l| format!("--{l}"))
                .into_iter()
                .chain(arg.get_short().map(|s| format!("-{s}")))
            {
                out.push((path.to_string(), flag));
            }
        }
        for sub in cmd
            .get_subcommands()
            .filter(|s| !s.is_hide_set() && s.get_name() != "help")
        {
            out.push((path.to_string(), sub.get_name().to_string()));
            walk(sub, &format!("{path} {}", sub.get_name()), out);
        }
    }
    let mut expected = Vec::new();
    walk(&Cli::command(), "git-loom", &mut expected);

    let scripts = [
        ("bash", include_str!("completions/git-loom.bash")),
        ("zsh", include_str!("completions/git-loom.zsh")),
        ("fish", include_str!("completions/git-loom.fish")),
        ("powershell", include_str!("completions/git-loom.ps1")),
        ("clink", include_str!("completions/git-loom.lua")),
    ];
    let mut missing = Vec::new();
    for (shell, script) in scripts {
        // fish spells `-a --all` as `-s a -l all`; longs first, for `-s l`.
        let script = if shell == "fish" {
            script.replace(" -l ", " --").replace(" -s ", " -")
        } else {
            script.to_string()
        };
        let words: std::collections::HashSet<&str> = script
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .collect();
        for (path, word) in &expected {
            if !words.contains(word.as_str()) {
                missing.push(format!("{shell}: {path} {word}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "missing from completions:\n{}",
        missing.join("\n")
    );
}

/// `GROUPED_COMMANDS` copies clap's command list by hand and had drifted: it
/// still offered the `c`/`a` aliases #380 removed, and not `cont`.
#[test]
fn grouped_help_lists_each_command_with_its_aliases() {
    let plain = apply_styles(GROUPED_COMMANDS, &help_styles(ThemeMode::Dark, false));
    // `  name, alias      Description`: the name column ends at two spaces.
    let rows: Vec<(&str, &str)> = plain
        .lines()
        .filter_map(|line| line.strip_prefix("  ")?.split_once("  "))
        .collect();
    let cli = Cli::command();
    let commands: Vec<_> = cli
        .get_subcommands()
        .filter(|s| !s.is_hide_set() && s.get_name() != "help")
        .collect();
    assert_eq!(rows.len(), commands.len(), "one row per command:\n{plain}");
    for cmd in commands {
        let name = cmd.get_name();
        let (names, description) = rows
            .iter()
            .find(|(names, _)| names.split(", ").next() == Some(name))
            .unwrap_or_else(|| panic!("no row for `{name}`"));
        let aliases = cmd.get_visible_aliases().collect::<Vec<_>>().join(", ");
        // Too many for the column, fold's trail its description.
        let shown = match names.split_once(", ") {
            Some((_, shown)) => shown,
            None if !aliases.is_empty() => description
                .rsplit_once(" [")
                .map_or("", |(_, shown)| shown.trim_end_matches(']')),
            None => "",
        };
        assert_eq!(shown, aliases, "aliases of `{name}`");
    }
}

/// `commit -l` targets the integration branch and `-b` a feature branch;
/// accepting both would silently drop one of them.
#[test]
fn commit_rejects_loose_with_branch() {
    let parse = |args: &[&str]| Cli::try_parse_from(["git-loom"].iter().chain(args).copied());
    assert!(parse(&["commit", "-l", "-m", "m"]).is_ok());
    assert!(parse(&["commit", "-b", "feature-a", "-m", "m"]).is_ok());
    assert!(parse(&["commit", "-l", "-b", "feature-a", "-m", "m"]).is_err());
    assert!(parse(&["commit", "-i", "-b", "feature-a", "-m", "m"]).is_err());
}

/// `-i` predates `-l`/`--loose` (#373) and stays as a hidden alias.
#[test]
fn commit_accepts_hidden_i_alias() {
    let parse = |args: &[&str]| Cli::try_parse_from(["git-loom"].iter().chain(args).copied());
    for flag in ["-i", "--loose"] {
        match parse(&["commit", flag, "-m", "m"]).unwrap().command {
            Some(Command::Commit { loose, .. }) => assert!(loose, "{flag}"),
            _ => panic!("{flag}: not a commit"),
        }
    }
}

/// Issue #380: one letter was easy to hit by mistake, and the TUI tree binds
/// `c`/`a` to commit/absorb.
#[test]
fn continue_and_abort_have_no_one_letter_alias() {
    let parse = |args: &[&str]| Cli::try_parse_from(["git-loom"].iter().chain(args).copied());
    for name in ["continue", "cont"] {
        assert!(
            matches!(parse(&[name]).unwrap().command, Some(Command::Continue)),
            "{name}"
        );
    }
    assert!(matches!(
        parse(&["abort"]).unwrap().command,
        Some(Command::Abort)
    ));
    assert!(parse(&["c"]).is_err());
    assert!(parse(&["a"]).is_err());
}

/// Issue #375: these were accepted only before the subcommand.
#[test]
fn color_flags_are_accepted_after_the_subcommand() {
    let cli = Cli::try_parse_from(["git-loom", "status", "--no-color", "--theme", "light"])
        .expect("global flags after the subcommand");
    assert!(cli.no_color);
    assert!(matches!(cli.theme, ThemeArg::Light));
    assert!(Cli::try_parse_from(["git-loom", "worktree", "list", "--theme=dark"]).is_ok());
}

#[test]
fn cli_help_renders() {
    // Catches template/marker mistakes that only clap's renderer would reject.
    let styles = help_styles(ThemeMode::Light, false);
    let help = Cli::command()
        .styles(styles.clone())
        .about(apply_styles(ABOUT, &styles))
        .after_help(apply_styles(GROUPED_COMMANDS, &styles))
        .help_template(apply_styles(HELP_TEMPLATE, &styles))
        .color(clap::ColorChoice::Never)
        .render_help()
        .to_string();
    assert_no_markers(&help);
    assert!(help.contains("Weave your branches together"));
    assert!(help.contains("Workflow:"));
    assert!(help.contains("Options:"));
    assert!(!help.contains('\u{1b}'));
}
