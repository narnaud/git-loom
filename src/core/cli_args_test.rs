use super::{GitArgs, split};

fn toks(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn run(args: &[&str], loom_flags: &[&str]) -> GitArgs {
    split(&toks(args), loom_flags)
}

#[test]
fn empty_input() {
    assert_eq!(run(&[], &[]), GitArgs::default());
}

#[test]
fn targets_only() {
    let out = run(&["ab", "d0..3a", "src/main.rs"], &[]);
    assert_eq!(out.targets, toks(&["ab", "d0..3a", "src/main.rs"]));
    assert!(out.options.is_empty());
}

#[test]
fn options_before_and_after_targets() {
    let out = run(&["--stat", "ab", "-w"], &[]);
    assert_eq!(out.options, toks(&["--stat", "-w"]));
    assert_eq!(out.targets, toks(&["ab"]));
}

#[test]
fn option_with_attached_value_stays_one_token() {
    let out = run(&["-U5", "--unified=5", "--format=%H", "ab"], &[]);
    assert_eq!(out.options, toks(&["-U5", "--unified=5", "--format=%H"]));
    assert_eq!(out.targets, toks(&["ab"]));
}

#[test]
fn detached_option_value_becomes_a_target() {
    // Loom keeps no table of value-taking git options, so `5` is a target and
    // the command reports it rather than guessing.
    let out = run(&["-U", "5"], &[]);
    assert_eq!(out.options, toks(&["-U"]));
    assert_eq!(out.targets, toks(&["5"]));
}

#[test]
fn double_dash_starts_the_pathspec() {
    let out = run(&["--stat", "ab", "--", "src/main.rs", "-weird-name"], &[]);
    assert_eq!(out.options, toks(&["--stat"]));
    assert_eq!(out.targets, toks(&["ab"]));
    assert_eq!(out.pathspec, toks(&["src/main.rs", "-weird-name"]));
}

#[test]
fn trailing_double_dash_yields_empty_pathspec() {
    let out = run(&["ab", "--"], &[]);
    assert_eq!(out.targets, toks(&["ab"]));
    assert!(out.pathspec.is_empty());
}

#[test]
fn loom_flags_are_recaptured_not_forwarded() {
    let out = run(&["ab", "--staged", "--stat"], &["--staged", "--cached"]);
    assert_eq!(out.loom_flags, toks(&["--staged"]));
    assert_eq!(out.options, toks(&["--stat"]));
    assert_eq!(out.targets, toks(&["ab"]));
}

#[test]
fn loom_flag_matching_ignores_an_attached_value() {
    let out = run(&["--staged=yes"], &["--staged"]);
    assert_eq!(out.loom_flags, toks(&["--staged=yes"]));
    assert!(out.options.is_empty());
}

#[test]
fn short_loom_flag_is_recaptured() {
    let out = run(&["ma", "-a"], &["-a", "--all"]);
    assert_eq!(out.loom_flags, toks(&["-a"]));
    assert!(out.options.is_empty());
}

#[test]
fn bare_hyphen_is_a_target() {
    let out = run(&["-"], &[]);
    assert_eq!(out.targets, toks(&["-"]));
    assert!(out.options.is_empty());
}

#[test]
fn loom_flags_after_the_separator_are_pathspec() {
    let out = run(&["--", "--staged"], &["--staged"]);
    assert!(out.loom_flags.is_empty());
    assert_eq!(out.pathspec, toks(&["--staged"]));
}
