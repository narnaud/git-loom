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
