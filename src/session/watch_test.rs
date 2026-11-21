use super::*;

#[test]
fn a_changed_fingerprint_is_due_at_the_next_poll_only() {
    let t0 = Instant::now();
    let mut watch = Watch::new(1, true, t0);
    assert!(!watch.due(t0 + INTERVAL / 2, || 2), "before the first poll");
    assert!(!watch.due(t0 + INTERVAL, || 1), "unchanged");
    assert!(
        !watch.due(t0 + INTERVAL * 3 / 2, || 2),
        "within the interval"
    );
    assert!(watch.due(t0 + INTERVAL * 2, || 2));
}

#[test]
fn a_load_records_its_fingerprint_and_restarts_the_interval() {
    let t0 = Instant::now();
    let mut watch = Watch::new(1, true, t0);
    watch.loaded(2, t0 + INTERVAL);
    assert!(
        !watch.due(t0 + INTERVAL * 3 / 2, || 3),
        "within the interval"
    );
    assert!(!watch.due(t0 + INTERVAL * 2, || 2), "own change");
    assert!(watch.due(t0 + INTERVAL * 4, || 3));
}

#[test]
fn an_off_watch_keeps_its_fingerprint_for_when_it_is_turned_on() {
    let t0 = Instant::now();
    let mut watch = Watch::new(1, false, t0);
    assert!(!watch.due(t0 + INTERVAL * 2, || 2), "off never polls");
    assert!(watch.toggle(t0 + INTERVAL * 2));
    assert!(!watch.due(t0 + INTERVAL * 5 / 2, || 2), "interval restarts");
    assert!(
        watch.due(t0 + INTERVAL * 3, || 2),
        "differs from the kept one"
    );
}

#[test]
fn fs_stamp_changes_with_head_refs_and_the_common_dir() {
    let dir = tempfile::tempdir().unwrap();
    let common = dir.path().join("common");
    let git = common.join("worktrees/w");
    std::fs::create_dir_all(common.join("refs/heads")).unwrap();
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(git.join("commondir"), "../..\n").unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/x").unwrap();
    std::fs::write(common.join("refs/heads/x"), "1\n").unwrap();

    let before = fs_stamp(&git);
    assert!(
        before
            .iter()
            .any(|(p, s)| p.ends_with("refs/heads/x") && s.is_some())
    );
    assert_eq!(before, fs_stamp(&git));

    std::fs::write(common.join("refs/heads/x"), "22\n").unwrap();
    assert_ne!(before, fs_stamp(&git), "ref in the common dir");
    let after_ref = fs_stamp(&git);
    let head = git.join("HEAD");
    std::fs::write(&head, "ref: refs/heads/yy").unwrap();
    assert_ne!(after_ref, fs_stamp(&git), "HEAD");

    let set_mtime = |path: &Path, secs| {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    };
    let ref_x = common.join("refs/heads/x");
    set_mtime(&ref_x, 1_000);
    let same_mtime = fs_stamp(&git);
    std::fs::write(&ref_x, "33\n").unwrap();
    set_mtime(&ref_x, 1_000);
    assert_ne!(
        same_mtime,
        fs_stamp(&git),
        "same length and mtime, other ref"
    );
    let with_x = fs_stamp(&git);
    std::fs::remove_file(&ref_x).unwrap();
    assert_ne!(with_x, fs_stamp(&git), "deleted ref");

    let index = git.join("index");
    std::fs::write(&index, "a").unwrap();
    let before_index = fs_stamp(&git);
    std::fs::write(&index, "b").unwrap();
    set_mtime(&index, 2_000);
    assert_ne!(before_index, fs_stamp(&git), "same length, other mtime");
}
