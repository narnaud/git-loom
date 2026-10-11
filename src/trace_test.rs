#[test]
fn classify_log_tells_headers_by_their_rule() {
    let rule = "=".repeat(80);
    let appended = "-".repeat(80);
    let content = format!(
        "[t] loom drop a1\n{rule}\n\n  [git] rebase  [5ms] FAILED\n    [stderr]\nboom\n\n  [git] status  [1ms]\n    [todo]\n[not a header]\n\n[t] loom continue\n{appended}\n"
    );
    use super::TraceLine::*;
    assert_eq!(
        super::classify_log(&content),
        vec![
            Header("[t] loom drop a1"),
            Rule(&rule),
            Blank,
            Command {
                text: "  [git] rebase  [5ms]",
                failed: true
            },
            Stderr("    [stderr]"),
            Stderr("boom"),
            Blank,
            Command {
                text: "  [git] status  [1ms]",
                failed: false
            },
            Label("    [todo]"),
            Content("[not a header]"),
            Blank,
            Header("[t] loom continue"),
            Rule(&appended),
        ]
    );
}

#[test]
fn classify_log_keeps_stderr_across_blank_lines_and_rules() {
    let rule = "=".repeat(80);
    let content = format!(
        "[t] loom push
{rule}

  [git] push  [5ms] FAILED
    [stderr]
hint: one

hint: two
{rule}

{rule}
"
    );
    use super::TraceLine::*;
    assert_eq!(
        super::classify_log(&content),
        vec![
            Header("[t] loom push"),
            Rule(&rule),
            Blank,
            Command {
                text: "  [git] push  [5ms]",
                failed: true
            },
            Stderr("    [stderr]"),
            Stderr("hint: one"),
            Blank,
            Stderr("hint: two"),
            Stderr(&rule),
            Blank,
            Stderr(&rule),
        ]
    );
}
