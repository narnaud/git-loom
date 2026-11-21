use super::*;

#[test]
fn the_window_is_looked_for_beside_the_cli() {
    let bin = tempfile::tempdir().unwrap();
    let cli = bin.path().join("git-loom");
    let gui = bin
        .path()
        .join(format!("git-loom-gui{}", std::env::consts::EXE_SUFFIX));

    assert_eq!(gui_beside(&cli), None, "not installed");
    assert_eq!(
        start(&cli).unwrap_err().to_string(),
        format!(
            "`git-loom-gui` is not installed beside `git-loom`, in `{}`\n\
             `cargo install git-loom --features gui` installs both, in cargo's directory",
            bin.path().display()
        )
    );

    std::fs::create_dir(&gui).unwrap();
    assert_eq!(gui_beside(&cli), None, "a directory is no executable");

    std::fs::remove_dir(&gui).unwrap();
    std::fs::write(&gui, "").unwrap();
    assert_eq!(gui_beside(&cli), Some(gui));
}

#[test]
fn a_window_that_does_not_start_says_why() {
    let bin = tempfile::tempdir().unwrap();
    let gui = bin
        .path()
        .join(format!("git-loom-gui{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&gui, "").unwrap();

    let error = start(&bin.path().join("git-loom")).unwrap_err().to_string();

    let start = format!("Failed to start `{}`: ", gui.display());
    assert!(error.starts_with(&start), "{error}");
    assert!(error.len() > start.len(), "no reason: {error}");
}

/// `current_exe` gives the link itself on macOS, the file it leads to on
/// Linux: the window installed beside the file is found either way.
#[cfg(unix)]
#[test]
fn the_window_is_found_beside_the_file_a_linked_cli_leads_to() {
    let real = tempfile::tempdir().unwrap();
    let linked = tempfile::tempdir().unwrap();
    let cli = real.path().join("git-loom");
    let gui = real.path().join("git-loom-gui");
    std::fs::write(&cli, "").unwrap();
    std::fs::write(&gui, "").unwrap();
    let link = linked.path().join("git-loom");
    std::os::unix::fs::symlink(&cli, &link).unwrap();

    assert_eq!(gui_beside(&link), Some(gui.canonicalize().unwrap()));
}
