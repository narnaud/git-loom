use super::*;

fn entry(lines: &[(Level, &str)]) -> LogEntry {
    LogEntry {
        command: "loom drop a b".to_string(),
        lines: lines
            .iter()
            .map(|(level, text)| (*level, text.to_string()))
            .collect(),
    }
}

#[test]
fn an_action_that_reported_nothing_is_done() {
    assert_eq!(entry(&[]).summary(), Summary::Done);
    assert_eq!(entry(&[(Level::Plain, "a note")]).summary(), Summary::Done);
    assert_eq!(entry(&[(Level::Success, "")]).summary(), Summary::Done);
}

#[test]
fn the_last_success_stands_for_those_before_it() {
    let one = entry(&[(Level::Success, "Dropped `a`")]);
    let three = entry(&[
        (Level::Success, "Dropped `a`"),
        (Level::Plain, "a note"),
        (Level::Success, "Dropped `b`"),
        (Level::Success, "Dropped `c`\nand what it took along"),
    ]);

    assert_eq!(
        one.summary(),
        Summary::Succeeded {
            last: "Dropped `a`",
            more: 0
        }
    );
    assert_eq!(
        three.summary(),
        Summary::Succeeded {
            last: "Dropped `c`",
            more: 2
        }
    );
}

/// A command that paused, or a continue that stopped again, succeeds without
/// a `✓`.
#[test]
fn without_a_success_the_last_warning_speaks() {
    let paused = entry(&[
        (Level::Warn, "Something to know"),
        (Level::Plain, "a note"),
        (
            Level::Warn,
            "Paused on a conflict\nResolve it, then continue",
        ),
    ]);

    assert_eq!(paused.summary(), Summary::Warned("Paused on a conflict"));
}

#[test]
fn a_commit_of_hunks_is_logged_with_the_ids_it_hands_over() {
    let action = Action::CommitHunks {
        dest: CommitDest::Branch("feature".to_string()),
        message: "Add it\n\nWith a body".to_string(),
        hunks: HunkArgs::new(
            vec!["a.rs:1".to_string(), "b.rs:2".to_string()],
            Some("0123456789ab".to_string()),
        ),
    };

    assert_eq!(
        command_line(&action, |target| format!("<{target}>")),
        "loom commit -b <feature> -m 'Add it' -p --hunks 'a.rs:1' --hunks 'b.rs:2' \
         --hunks-from 0123456789ab"
    );
}

#[test]
fn a_success_is_shown_rather_than_a_warning() {
    let both = entry(&[
        (Level::Success, "Folded"),
        (Level::Warn, "Something to know"),
    ]);

    assert_eq!(
        both.summary(),
        Summary::Succeeded {
            last: "Folded",
            more: 0
        }
    );
}
