/// Shared test utilities for creating and manipulating test repositories.
use git2::{BranchType, Repository, Signature};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::TempDir;

use crate::core::graph;
use crate::core::repo::{BranchInfo, CommitInfo, RemoteStatus, RepoInfo, UpstreamInfo};
use crate::core::shortid::IdAllocator;
use crate::core::status_json;

/// Global mutex to serialize `in_dir` calls: `set_current_dir` mutates
/// process-global state, and Cargo runs tests in parallel threads.
static IN_DIR_LOCK: Mutex<()> = Mutex::new(());

/// Whether `output` mentions `path`.
///
/// Compares with forward slashes: git prints paths that way on Windows too,
/// while `Path` there renders them with backslashes.
pub fn mentions_path(output: &str, path: &Path) -> bool {
    let wanted = path.display().to_string().replace('\\', "/");
    output.replace('\\', "/").contains(&wanted)
}

/// A test repository wrapper with convenient helper methods.
pub struct TestRepo {
    pub repo: Repository,
    _dir: TempDir,
}

impl TestRepo {
    /// Create a new test repository with an initial commit.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        Self::configure_identity(&repo);

        {
            let sig = Self::sig();
            let tree_id = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        TestRepo { repo, _dir: dir }
    }

    /// Create a test repository without any initial commit (empty).
    pub fn new_empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        Self::configure_identity(&repo);
        TestRepo { repo, _dir: dir }
    }

    /// Create a test repository with a bare "remote" at remote.git, a clone of it
    /// with an initial commit on `main`, and an `integration` branch tracking
    /// `origin/main` — a typical development setup.
    pub fn new_with_remote() -> Self {
        let dir = tempfile::tempdir().unwrap();

        let remote_path = dir.path().join("remote.git");
        let remote_repo = Repository::init_bare(&remote_path).unwrap();
        // Ensure HEAD points to main regardless of system default
        remote_repo.set_head("refs/heads/main").unwrap();

        {
            let sig = Self::sig();
            let tree_id = {
                let mut index = remote_repo.index().unwrap();
                index.write_tree().unwrap()
            };
            let tree = remote_repo.find_tree(tree_id).unwrap();
            remote_repo
                .commit(Some("refs/heads/main"), &sig, &sig, "Initial", &tree, &[])
                .unwrap();
        }

        let work_path = dir.path().join("work");
        let repo = Repository::clone(remote_path.to_str().unwrap(), &work_path).unwrap();
        Self::configure_identity(&repo);

        {
            let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("integration", &head_commit, false).unwrap();
            repo.set_head("refs/heads/integration").unwrap();

            let mut integration = repo.find_branch("integration", BranchType::Local).unwrap();
            integration.set_upstream(Some("origin/main")).unwrap();
        }

        TestRepo { repo, _dir: dir }
    }

    /// Like `new_with_remote`, but stays on the `main` branch instead of
    /// creating a separate `integration` branch. Useful for testing the
    /// loose-commit path where the branch name must match the upstream's
    /// local name.
    pub fn new_on_main_with_remote() -> Self {
        let dir = tempfile::tempdir().unwrap();

        let remote_path = dir.path().join("remote.git");
        let remote_repo = Repository::init_bare(&remote_path).unwrap();
        remote_repo.set_head("refs/heads/main").unwrap();

        {
            let sig = Self::sig();
            let tree_id = {
                let mut index = remote_repo.index().unwrap();
                index.write_tree().unwrap()
            };
            let tree = remote_repo.find_tree(tree_id).unwrap();
            remote_repo
                .commit(Some("refs/heads/main"), &sig, &sig, "Initial", &tree, &[])
                .unwrap();
        }

        let work_path = dir.path().join("work");
        let repo = Repository::clone(remote_path.to_str().unwrap(), &work_path).unwrap();
        Self::configure_identity(&repo);
        // After clone, HEAD is already on `main` tracking `origin/main`.

        TestRepo { repo, _dir: dir }
    }

    /// Configure user identity in a repo so shell-invoked git commands work
    /// even when no global git config is present.
    fn configure_identity(repo: &Repository) {
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@test.com").unwrap();
        // loom must suppress the editor itself (`git merge --continue` is a
        // `git commit` with no `--no-edit`), so an editor that leaks through
        // fails the test instead of hanging it. A `GIT_EDITOR` in the
        // environment overrides this; the default lives in repo config because
        // the process env is shared by every test running in parallel.
        config.set_str("core.editor", "false").unwrap();
        // Tests write their fixtures with LF and compare the bytes back. A
        // stock Windows install sets `core.autocrlf=true`, which rewrites every
        // file git itself puts in the working tree — a checkout, a reset, a
        // three-way apply — as CRLF, and then reports those files as modified
        // against an index that holds LF. Pin it so a fixture means the same
        // thing on every platform.
        config.set_str("core.autocrlf", "false").unwrap();
        // Hooks a test installs go in this repository's `.git/hooks`; a
        // `core.hooksPath` in the user's config would leave them all inert, and
        // the test passing for the wrong reason.
        let hooks = repo.path().join("hooks").display().to_string();
        config
            .set_str("core.hooksPath", &hooks.replace('\\', "/"))
            .unwrap();
    }

    /// Get the signature used for commits.
    fn sig() -> Signature<'static> {
        Signature::now("Test", "test@test.com").unwrap()
    }

    /// Create a commit that writes `filename`.
    pub fn commit(&self, message: &str, filename: &str) -> git2::Oid {
        self.commit_with_sig(message, filename, &Self::sig())
    }

    /// Like [`Self::commit`], but with a fixed committer time.
    ///
    /// Lets a test build the committer-time ties it means to test instead of
    /// depending on how fast the machine ran.
    pub fn commit_at(&self, message: &str, filename: &str, seconds: i64) -> git2::Oid {
        let time = git2::Time::new(seconds, 0);
        let sig = Signature::new("Test", "test@test.com", &time).unwrap();
        self.commit_with_sig(message, filename, &sig)
    }

    /// Write `filename`, stage it, and commit it onto HEAD with `sig`.
    fn commit_with_sig(&self, message: &str, filename: &str, sig: &Signature) -> git2::Oid {
        let path = self.repo.workdir().unwrap().join(filename);
        fs::write(&path, message).unwrap();

        let mut index = self.repo.index().unwrap();
        index.add_path(Path::new(filename)).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = self.repo.find_tree(tree_id).unwrap();

        match self.repo.head() {
            Ok(head) => {
                let parent = self.repo.find_commit(head.target().unwrap()).unwrap();
                self.repo
                    .commit(Some("HEAD"), sig, sig, message, &tree, &[&parent])
                    .unwrap()
            }
            Err(_) => self
                .repo
                .commit(Some("HEAD"), sig, sig, message, &tree, &[])
                .unwrap(),
        }
    }

    /// Create a commit without changing files (reusing the current tree).
    pub fn commit_empty(&self, message: &str) -> git2::Oid {
        let sig = Self::sig();
        let tree_id = {
            let mut index = self.repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = self.repo.find_tree(tree_id).unwrap();

        if let Ok(head) = self.repo.head() {
            let parent = self.repo.find_commit(head.target().unwrap()).unwrap();
            self.repo
                .commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
                .unwrap()
        } else {
            self.repo
                .commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
                .unwrap()
        }
    }

    /// Create a merge commit combining two parent commits.
    pub fn commit_merge(
        &self,
        message: &str,
        parent1_oid: git2::Oid,
        parent2_oid: git2::Oid,
    ) -> git2::Oid {
        let sig = Self::sig();
        let p1 = self.repo.find_commit(parent1_oid).unwrap();
        let p2 = self.repo.find_commit(parent2_oid).unwrap();
        let tree = self.repo.find_tree(p1.tree_id()).unwrap();
        self.repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &[&p1, &p2])
            .unwrap()
    }

    /// Get a commit relative to HEAD (0 = HEAD, 1 = HEAD~1, …).
    pub fn get_commit(&self, steps_back: usize) -> git2::Commit<'_> {
        let mut commit = self.repo.head().unwrap().peel_to_commit().unwrap();
        for _ in 0..steps_back {
            commit = commit.parent(0).unwrap();
        }
        commit
    }

    /// Get the HEAD commit.
    pub fn head_commit(&self) -> git2::Commit<'_> {
        self.get_commit(0)
    }

    /// Get the commit message at a position relative to HEAD.
    pub fn get_message(&self, steps_back: usize) -> String {
        self.get_commit(steps_back)
            .message()
            .unwrap()
            .trim()
            .to_string()
    }

    /// Get the first line of the commit message at a position relative to HEAD.
    pub fn get_subject(&self, steps_back: usize) -> String {
        crate::core::repo::commit_subject(&self.get_commit(steps_back))
    }

    /// Get the OID of a commit relative to HEAD.
    pub fn get_oid(&self, steps_back: usize) -> git2::Oid {
        self.get_commit(steps_back).id()
    }

    /// Create a branch at the current HEAD.
    pub fn create_branch(&self, name: &str) -> git2::Branch<'_> {
        let head_commit = self.head_commit();
        self.repo.branch(name, &head_commit, false).unwrap()
    }

    /// Get the path to the working directory.
    pub fn workdir(&self) -> PathBuf {
        self.repo.workdir().unwrap().to_path_buf()
    }

    /// Run a closure with the current directory set to the repo's working
    /// directory, under a global mutex and a drop guard so concurrent test
    /// threads cannot corrupt each other's cwd even on a panic.
    pub fn in_dir<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _lock = IN_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let restore = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        std::env::set_current_dir(self.workdir()).unwrap();

        struct RestoreDir(PathBuf);
        impl Drop for RestoreDir {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _guard = RestoreDir(restore);

        f()
    }

    /// Run a closure with CWD set to the given path (must be inside the repo).
    pub fn in_dir_path<F, R>(&self, path: &Path, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _lock = IN_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let restore = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        std::env::set_current_dir(path).unwrap();
        struct RestoreDir(PathBuf);
        impl Drop for RestoreDir {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _guard = RestoreDir(restore);
        f()
    }

    /// Switch HEAD to a branch and update the working directory.
    pub fn switch_branch(&self, name: &str) {
        self.repo.set_head(&format!("refs/heads/{}", name)).unwrap();
        self.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    }

    /// Hard-reset the current branch to a specific commit.
    pub fn reset_hard(&self, oid: git2::Oid) {
        let commit = self.find_commit(oid);
        self.repo
            .reset(commit.as_object(), git2::ResetType::Hard, None)
            .unwrap();
    }

    /// Force-update the working directory to match HEAD.
    pub fn force_checkout(&self) {
        self.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    }

    /// Create a branch at HEAD that tracks a remote upstream.
    pub fn create_branch_tracking(&self, name: &str, upstream: &str) {
        let mut branch = self.create_branch(name);
        branch.set_upstream(Some(upstream)).unwrap();
    }

    /// Write content to a file in the working directory (without committing).
    pub fn write_file(&self, filename: &str, content: &str) {
        let path = self.workdir().join(filename);
        fs::write(path, content).unwrap();
    }

    /// Read content from a file in the working directory.
    pub fn read_file(&self, filename: &str) -> String {
        let path = self.workdir().join(filename);
        fs::read_to_string(path).unwrap()
    }

    /// Check if HEAD is on a branch.
    pub fn is_on_branch(&self) -> bool {
        self.repo.head().unwrap().is_branch()
    }

    /// Get the current branch name (shorthand).
    pub fn current_branch_name(&self) -> String {
        self.repo.head().unwrap().shorthand().unwrap().to_string()
    }

    /// Check if a branch exists.
    pub fn branch_exists(&self, name: &str) -> bool {
        self.repo.find_branch(name, BranchType::Local).is_ok()
    }

    /// Delete a local branch (including its tracking config).
    pub fn delete_branch(&self, name: &str) {
        let workdir = self.workdir();
        crate::git::branch_delete(workdir.as_path(), name).unwrap();
    }

    /// Get the current HEAD commit OID.
    pub fn head_oid(&self) -> git2::Oid {
        self.repo.head().unwrap().target().unwrap()
    }

    /// Find a commit by OID.
    pub fn find_commit(&self, oid: git2::Oid) -> git2::Commit<'_> {
        self.repo.find_commit(oid).unwrap()
    }

    /// Create a branch at a specific commit.
    pub fn create_branch_at_commit(&self, name: &str, oid: git2::Oid) -> git2::Branch<'_> {
        let commit = self.find_commit(oid);
        self.repo.branch(name, &commit, false).unwrap()
    }

    /// Get the target OID of a remote branch (e.g. "origin/main").
    pub fn find_remote_branch_target(&self, name: &str) -> git2::Oid {
        self.repo
            .find_branch(name, BranchType::Remote)
            .unwrap()
            .get()
            .target()
            .unwrap()
    }

    /// Get the target OID of a branch. Panics if it does not exist.
    pub fn get_branch_target(&self, name: &str) -> git2::Oid {
        self.repo
            .find_branch(name, BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap()
    }

    /// Set HEAD to a detached state at a specific commit.
    pub fn set_detached_head(&self, oid: git2::Oid) {
        self.repo.set_head_detached(oid).unwrap();
    }

    /// Set up a fake editor that replaces commit messages. Per repository
    /// (`core.editor`), so tests running at the same time each keep their own
    /// message.
    ///
    /// Only reaches `run_git_interactive` commands: `run_git_captured` sets
    /// `GIT_EDITOR=true` itself, so captured commands ignore it.
    pub fn set_fake_editor(&self, new_message: &str) {
        // Git on Windows uses Git Bash, so we use the same shell command format for all platforms
        let editor_script = format!("sh -c 'echo \"{}\" > \"$1\"' --", new_message);
        self.set_config("core.editor", &editor_script);
    }

    /// Path to the remote repository, `None` without a remote.git setup.
    pub fn remote_path(&self) -> Option<PathBuf> {
        let remote_path = self._dir.path().join("remote.git");
        if remote_path.exists() {
            Some(remote_path)
        } else {
            None
        }
    }

    /// Add commits to the remote's main branch, simulating upstream changes.
    /// Returns the OID of the last one.
    pub fn add_remote_commits(&self, messages: &[&str]) -> git2::Oid {
        let remote_path = self.remote_path().expect("No remote repository found");
        let remote_repo = Repository::open_bare(&remote_path).unwrap();

        let sig = Self::sig();
        let mut last_oid = remote_repo
            .find_branch("main", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();

        for message in messages {
            let parent = remote_repo.find_commit(last_oid).unwrap();
            let tree = parent.tree().unwrap();
            last_oid = remote_repo
                .commit(
                    Some("refs/heads/main"),
                    &sig,
                    &sig,
                    message,
                    &tree,
                    &[&parent],
                )
                .unwrap();
        }

        last_oid
    }

    /// Simulate a cherry-pick of a local commit onto the remote's main branch.
    ///
    /// Computes the diff between the local commit and its parent, then applies
    /// those new/changed files on top of the remote tip's tree. This produces a
    /// commit with the same patch-id as the original, so git rebase will
    /// correctly detect it as a duplicate.
    pub fn cherry_pick_to_remote(&self, local_oid: git2::Oid, message: &str) -> git2::Oid {
        let remote_path = self.remote_path().expect("No remote repository found");
        let remote_repo = Repository::open_bare(&remote_path).unwrap();
        // Use a different committer to ensure the cherry-picked commit gets a
        // different OID from the original (same patch-id, different commit hash).
        let sig = Signature::now("Upstream", "upstream@test.com").unwrap();

        let remote_tip = remote_repo
            .find_branch("main", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let parent = remote_repo.find_commit(remote_tip).unwrap();

        let local_commit = self.repo.find_commit(local_oid).unwrap();
        let local_tree = local_commit.tree().unwrap();
        let local_parent_tree = local_commit.parent(0).unwrap().tree().unwrap();
        let diff = self
            .repo
            .diff_tree_to_tree(Some(&local_parent_tree), Some(&local_tree), None)
            .unwrap();

        let remote_parent_tree = parent.tree().unwrap();
        let mut builder = remote_repo.treebuilder(Some(&remote_parent_tree)).unwrap();

        diff.foreach(
            &mut |delta, _| {
                match delta.status() {
                    git2::Delta::Added | git2::Delta::Modified => {
                        let new_file = delta.new_file();
                        let path = new_file.path().unwrap().to_str().unwrap();
                        let blob = self.repo.find_blob(new_file.id()).unwrap();
                        let new_blob = remote_repo.blob(blob.content()).unwrap();
                        builder.insert(path, new_blob, 0o100644).unwrap();
                    }
                    git2::Delta::Deleted => {
                        let old_file = delta.old_file();
                        let path = old_file.path().unwrap().to_str().unwrap();
                        builder.remove(path).unwrap();
                    }
                    _ => {}
                }
                true
            },
            None,
            None,
            None,
        )
        .unwrap();

        let tree_oid = builder.write().unwrap();
        let tree = remote_repo.find_tree(tree_oid).unwrap();

        remote_repo
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                message,
                &tree,
                &[&parent],
            )
            .unwrap()
    }

    /// Fetch from the remote repository, updating origin/* references.
    pub fn fetch_remote(&self) {
        self.repo
            .find_remote("origin")
            .unwrap()
            .fetch(&["main"], None, None)
            .unwrap();
    }

    /// Fast-forward the remote's `main` to a local branch tip.
    ///
    /// Simulates a feature branch landing upstream unchanged (a fast-forward
    /// merge of a pull request): `origin/main` ends up at the exact same OID
    /// as the local branch.
    pub fn push_branch_to_remote_main(&self, branch: &str) {
        crate::git::run_git(
            self.workdir().as_path(),
            &["push", "origin", &format!("{}:main", branch)],
        )
        .unwrap();
        self.fetch_remote();
    }

    /// Create a branch at a specific commit hash.
    pub fn create_branch_at(&self, name: &str, commit_hash: &str) {
        crate::git::branch_create(self.workdir().as_path(), name, commit_hash).unwrap();
    }

    /// Merge a branch into the current branch with --no-ff.
    pub fn merge_no_ff(&self, branch: &str) {
        let outcome =
            crate::git::merge_no_ff(self.workdir().as_path(), self.repo.path(), branch).unwrap();
        assert!(
            matches!(outcome, crate::git::MergeOutcome::Completed),
            "merge_no_ff: expected Completed, got Stopped"
        );
    }

    /// Stage files in the working directory.
    pub fn stage_files(&self, files: &[&str]) {
        crate::git::stage_files(self.workdir().as_path(), files).unwrap();
    }

    /// Commit already-staged files with a message.
    pub fn commit_staged(&self, message: &str) {
        crate::git::commit(self.workdir().as_path(), message).unwrap();
    }

    /// Get the names of files that differ from HEAD.
    pub fn diff_head_name_only(&self) -> String {
        crate::git::diff_head_name_only(self.workdir().as_path()).unwrap()
    }

    /// Get the diff of a single commit.
    pub fn diff_commit(&self, oid: &str) -> String {
        crate::git::diff_commit(self.workdir().as_path(), oid).unwrap()
    }

    /// Set a git config value.
    pub fn set_config(&self, key: &str, value: &str) {
        crate::git::run_git(self.workdir().as_path(), &["config", key, value]).unwrap();
    }

    /// Install an executable git hook and point `core.hooksPath` at this repo,
    /// which the user's own config may have aimed elsewhere.
    #[cfg(unix)]
    pub fn install_hook(&self, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        let hooks = self.repo.path().join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join(name);
        std::fs::write(&hook, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        self.set_config("core.hooksPath", hooks.to_str().unwrap());
    }

    /// Get porcelain status output.
    pub fn status_porcelain(&self) -> String {
        crate::git::run_git_stdout(self.workdir().as_path(), &["status", "--porcelain"]).unwrap()
    }

    /// Rebase commits between `upstream` and HEAD onto `newbase` with --update-refs.
    pub fn rebase_onto(&self, newbase: &str, upstream: &str) {
        crate::git::rebase_onto(self.workdir().as_path(), newbase, upstream).unwrap();
    }

    /// Get all non-merge commit messages between HEAD and merge-base.
    pub fn commit_messages(&self) -> Vec<String> {
        let info = crate::core::repo::gather_repo_info(&self.repo, false, 1).unwrap();
        info.commits.iter().map(|c| c.message.clone()).collect()
    }

    /// Get all branch names in the commit range.
    pub fn branch_names(&self) -> Vec<String> {
        let info = crate::core::repo::gather_repo_info(&self.repo, false, 1).unwrap();
        info.branches.iter().map(|b| b.name.clone()).collect()
    }

    /// Get the commit summary at the tip of a branch.
    pub fn branch_commit_summary(&self, name: &str) -> String {
        let oid = self.get_branch_target(name);
        let commit = self.find_commit(oid);
        commit.summary().ok().flatten().unwrap_or("").to_string()
    }

    /// Get the file paths changed in a commit.
    pub fn commit_file_paths(&self, oid: git2::Oid) -> Vec<String> {
        crate::core::repo::commit_file_paths(&self.repo, oid).unwrap()
    }

    /// True if `path` exists in the tree of `oid`.
    pub fn commit_has_file(&self, oid: git2::Oid, path: &str) -> bool {
        self.find_commit(oid)
            .tree()
            .unwrap()
            .get_path(Path::new(path))
            .is_ok()
    }

    /// Create a commit touching multiple `(filename, content)` files at once.
    pub fn commit_multi(&self, files: &[(&str, &str)], message: &str) -> git2::Oid {
        for (filename, content) in files {
            self.write_file(filename, content);
        }
        let mut index = self.repo.index().unwrap();
        for (filename, _) in files {
            index.add_path(Path::new(filename)).unwrap();
        }
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = self.repo.find_tree(tree_id).unwrap();
        let sig = Self::sig();

        if let Ok(head) = self.repo.head() {
            let parent = self.repo.find_commit(head.target().unwrap()).unwrap();
            self.repo
                .commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
                .unwrap()
        } else {
            self.repo
                .commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
                .unwrap()
        }
    }

    /// Create a real submodule checkout at `path`, with two commits of its own,
    /// and stage the first of them as the gitlink.
    ///
    /// Hand-built rather than `git submodule add`, which wants a clone and
    /// `protocol.file.allow`. Returns the nested repository's two commits.
    pub fn add_submodule(&self, path: &str) -> (git2::Oid, git2::Oid) {
        let sub = Repository::init(self.workdir().join(path)).unwrap();
        Self::configure_identity(&sub);
        let first = Self::commit_in(&sub, "sub one");
        let second = Self::commit_in(&sub, "sub two");
        Self::detach(&sub, first);
        // Without the `.gitmodules` entry `git add` only warns about an
        // embedded repository; with it the path is a submodule to every command.
        self.write_file(
            ".gitmodules",
            &format!("[submodule \"{path}\"]\n\tpath = {path}\n\turl = ./{path}\n"),
        );
        self.stage_files(&[".gitmodules", path]);
        (first, second)
    }

    /// Check the submodule at `path` out at `oid`, the way a user bumping a
    /// submodule pointer does.
    pub fn checkout_submodule(&self, path: &str, oid: git2::Oid) {
        Self::detach(&Repository::open(self.workdir().join(path)).unwrap(), oid);
    }

    /// The commit a submodule is pinned to in `oid`'s tree.
    pub fn submodule_oid(&self, oid: git2::Oid, path: &str) -> git2::Oid {
        self.find_commit(oid)
            .tree()
            .unwrap()
            .get_path(Path::new(path))
            .unwrap()
            .id()
    }

    /// Commit `message` in `repo`, as a file of its own.
    fn commit_in(repo: &Repository, message: &str) -> git2::Oid {
        let name = format!("{}.txt", message.replace(' ', "-"));
        fs::write(repo.workdir().unwrap().join(&name), message).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(&name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Self::sig();
        let parents: Vec<git2::Commit> = repo
            .head()
            .ok()
            .and_then(|head| head.target())
            .map(|oid| repo.find_commit(oid).unwrap())
            .into_iter()
            .collect();
        let parents: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .unwrap()
    }

    /// Detach `repo`'s HEAD at `oid` and check it out.
    fn detach(repo: &Repository, oid: git2::Oid) {
        repo.set_head_detached(oid).unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    }

    /// Assert that the working tree is clean (no diff from HEAD).
    pub fn assert_working_tree_clean(&self) {
        let diff = self.diff_head_name_only();
        assert!(
            diff.trim().is_empty(),
            "working tree should be clean, but has: {}",
            diff
        );
    }
}

/// Builder for creating test repositories with a fluent API.
pub struct TestRepoBuilder {
    repo: TestRepo,
}

impl TestRepoBuilder {
    /// Create a new builder with an initial empty repository.
    pub fn new() -> Self {
        TestRepoBuilder {
            repo: TestRepo::new_empty(),
        }
    }

    /// Create a new builder with an initial commit.
    pub fn with_initial_commit() -> Self {
        TestRepoBuilder {
            repo: TestRepo::new(),
        }
    }

    /// Add a commit with a file.
    pub fn commit(self, message: &str, filename: &str) -> Self {
        self.repo.commit(message, filename);
        self
    }

    /// Create a branch at the current HEAD.
    pub fn branch(self, name: &str) -> Self {
        self.repo.create_branch(name);
        self
    }

    /// Build and return the test repository.
    pub fn build(self) -> TestRepo {
        self.repo
    }
}

impl Default for TestRepoBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A repo where branch `alpha`'s only commit replays empty: the same content
/// reached the upstream through unrelated commits, so a weave rebase drops it
/// while git still stops at its `edit` line. Returns the repo and that commit.
///
/// The upstream gets there in two steps on purpose: no patch-id matches the
/// branch's, so only the emptiness at apply time reveals it.
pub fn repo_with_dropped_replay() -> (TestRepo, git2::Oid) {
    let (t, target) = branch_forked_below_upstream();

    publish_upstream_with_the_branch_content(&t);
    (t, target)
}

/// The same shape plus a commit on `alpha` that is still needed: rewriting that
/// one has to get past the redundant commit below it rather than refuse.
/// Returns the repo, the redundant commit and the one worth keeping.
pub fn repo_with_a_redundant_commit_below() -> (TestRepo, git2::Oid, git2::Oid) {
    let (t, redundant) = branch_forked_below_upstream();
    let keeper = t.commit_multi(&[("three.txt", "three\n")], "still needed");

    publish_upstream_with_the_branch_content(&t);
    (t, redundant, keeper)
}

/// A branch whose middle commit replays empty, between two that do not: the
/// shape a two-stop fold has to walk through. Returns the repo, the older
/// commit and the newer one.
pub fn repo_with_a_redundant_commit_between() -> (TestRepo, git2::Oid, git2::Oid) {
    let t = TestRepo::new_with_remote();
    let base = t.find_remote_branch_target("origin/main").to_string();

    t.create_branch_at("alpha", &base);
    t.switch_branch("alpha");
    let older = t.commit_multi(
        &[("moved.txt", "moved\n"), ("kept.txt", "kept\n")],
        "older change",
    );
    t.commit_multi(
        &[("one.txt", "final one\n"), ("two.txt", "final two\n")],
        "branch change",
    );
    let newer = t.commit_multi(&[("newer.txt", "newer\n")], "newer change");
    t.create_branch_at("upstream-work", &base);

    publish_upstream_with_the_branch_content(&t);
    (t, older, newer)
}

/// The redundant commit the other way round: above the one being rewritten, so
/// the rebase only meets it after the caller has done its work.
pub fn repo_with_a_redundant_commit_above() -> (TestRepo, git2::Oid) {
    let t = TestRepo::new_with_remote();
    let base = t.find_remote_branch_target("origin/main").to_string();

    t.create_branch_at("alpha", &base);
    t.switch_branch("alpha");
    let keeper = t.commit_multi(&[("three.txt", "three\n")], "still needed");
    t.commit_multi(
        &[("one.txt", "final one\n"), ("two.txt", "final two\n")],
        "branch change",
    );
    t.create_branch_at("upstream-work", &base);

    publish_upstream_with_the_branch_content(&t);
    (t, keeper)
}

/// Give the upstream the branch's final content through unrelated commits, then
/// weave `alpha` on top of it.
fn publish_upstream_with_the_branch_content(t: &TestRepo) {
    t.switch_branch("upstream-work");
    t.commit_multi(&[("one.txt", "first try\n")], "upstream step one");
    t.commit_multi(
        &[("one.txt", "final one\n"), ("two.txt", "final two\n")],
        "upstream step two",
    );

    weave_over_upstream(t);
}

/// The same shape, with the upstream carrying a literal cherry-pick of the
/// branch commit: author and message match, so the identity backstop alone
/// would take the base for the target's replay.
pub fn repo_with_cherry_picked_replay() -> (TestRepo, git2::Oid) {
    let (t, target) = branch_forked_below_upstream();

    t.switch_branch("upstream-work");
    // Somewhere else to land on: cherry-picking onto the target's own parent
    // would rebuild the very same commit.
    t.commit_multi(&[("unrelated.txt", "unrelated\n")], "upstream work");
    crate::git::run_git(t.workdir().as_path(), &["cherry-pick", &target.to_string()]).unwrap();

    weave_over_upstream(&t);
    (t, target)
}

/// Branch `alpha` with one two-file commit, and an empty `upstream-work`, both
/// forked at the upstream tip the caller then moves past.
fn branch_forked_below_upstream() -> (TestRepo, git2::Oid) {
    let t = TestRepo::new_with_remote();
    let base = t.find_remote_branch_target("origin/main").to_string();

    t.create_branch_at("alpha", &base);
    t.switch_branch("alpha");
    let target = t.commit_multi(
        &[("one.txt", "final one\n"), ("two.txt", "final two\n")],
        "branch change",
    );
    t.create_branch_at("upstream-work", &base);

    (t, target)
}

/// Publish `upstream-work` as the new upstream and weave `alpha` on top of it,
/// leaving `alpha` forked below the base it will be replayed onto.
fn weave_over_upstream(t: &TestRepo) {
    t.push_branch_to_remote_main("upstream-work");
    t.switch_branch("integration");
    t.reset_hard(t.find_remote_branch_target("origin/main"));
    t.merge_no_ff("alpha");
}

// ── In-memory status graphs ──

/// Drop ANSI escapes so rendered output can be compared as plain text.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for inner in chars.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A fake OID whose first byte is `byte`.
pub fn oid(byte: u8) -> git2::Oid {
    let mut bytes = [0u8; 20];
    bytes[0] = byte;
    git2::Oid::from_bytes(&bytes).unwrap()
}

pub fn commit(byte: u8, message: &str, parent: Option<u8>) -> CommitInfo {
    CommitInfo {
        oid: oid(byte),
        short_id: format!("{:07x}", byte),
        message: message.to_string(),
        change_id: None,
        parent_oid: parent.map(oid),
        files: vec![],
    }
}

pub fn branch(name: &str, tip: u8, remote: Option<RemoteStatus>) -> BranchInfo {
    BranchInfo {
        name: name.to_string(),
        tip_oid: oid(tip),
        remote,
    }
}

/// An `integration` branch at its upstream base `0xAA`, with nothing on it.
pub fn base_info() -> RepoInfo {
    RepoInfo {
        branch_name: "integration".to_string(),
        upstream: UpstreamInfo {
            label: "origin/main".to_string(),
            tip_oid: oid(0xAA),
            base_short_id: "aaa0000".to_string(),
            base_message: "Initial commit".to_string(),
            base_date: "2025-07-06".to_string(),
            commits_ahead: 0,
            merge_base_oid: oid(0xAA),
        },
        commits: vec![],
        branches: vec![],
        working_changes: vec![],
        context_commits: vec![],
    }
}

/// The JSON graph the way `status::run` builds it, with nothing hidden.
pub fn status_graph(info: RepoInfo) -> status_json::StatusGraph {
    status_graph_in(info, "")
}

/// [`status_graph`] run from the `cwd_prefix` subdirectory.
pub fn status_graph_in(info: RepoInfo, cwd_prefix: &str) -> status_json::StatusGraph {
    let ids = IdAllocator::new(info.collect_entities());
    let branch = info.branch_name.clone();
    let stacks = stack_edges(&info);
    status_json::build(
        &graph::build_sections(info),
        &branch,
        &stacks,
        &ids,
        cwd_prefix,
    )
}

/// Every branch's stack edge, with nothing hidden.
pub fn stack_edges(info: &RepoInfo) -> HashMap<String, status_json::StackEdge> {
    graph::stack_parents(info)
        .into_iter()
        .map(|(b, below)| {
            let edge = status_json::StackEdge {
                below: Some(below),
                below_hidden: false,
            };
            (b, edge)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_creation() {
        let repo = TestRepo::new();
        assert!(repo.is_on_branch());
        assert_eq!(repo.get_message(0), "Initial commit");
    }

    #[test]
    fn test_commit_and_get() {
        let repo = TestRepo::new();
        repo.commit("Second commit", "file2.txt");
        repo.commit("Third commit", "file3.txt");

        assert_eq!(repo.get_message(0), "Third commit");
        assert_eq!(repo.get_message(1), "Second commit");
        assert_eq!(repo.get_message(2), "Initial commit");
    }

    #[test]
    fn test_builder_pattern() {
        let repo = TestRepoBuilder::with_initial_commit()
            .commit("Second", "file2.txt")
            .commit("Third", "file3.txt")
            .branch("feature")
            .build();

        assert_eq!(repo.get_message(0), "Third");
        assert_eq!(repo.get_message(1), "Second");

        assert!(
            repo.repo
                .find_branch("feature", git2::BranchType::Local)
                .is_ok()
        );
    }

    #[test]
    fn test_file_operations() {
        let repo = TestRepo::new();
        repo.write_file("test.txt", "hello");
        assert_eq!(repo.read_file("test.txt"), "hello");
    }
}
