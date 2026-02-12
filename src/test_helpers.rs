/// Shared test utilities for git repository testing.
///
/// Provides a clean API for creating and manipulating test repositories,
/// reducing boilerplate in test code.
use git2::{BranchType, Repository, Signature};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

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

        // Create an initial commit
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
        TestRepo { repo, _dir: dir }
    }

    /// Create a test repository with a remote (bare repo) and an integration branch.
    ///
    /// Sets up:
    /// - A bare "remote" repository at remote.git
    /// - A cloned working repository
    /// - An initial commit on the main branch
    /// - An integration branch tracking origin/main
    ///
    /// This mimics a typical development setup with an upstream remote.
    pub fn new_with_remote() -> Self {
        let dir = tempfile::tempdir().unwrap();

        // Create a bare "remote"
        let remote_path = dir.path().join("remote.git");
        let remote_repo = Repository::init_bare(&remote_path).unwrap();

        // Create initial commit in the bare repo so it has a main branch
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

        // Clone it
        let work_path = dir.path().join("work");
        let repo = Repository::clone(remote_path.to_str().unwrap(), &work_path).unwrap();

        // Create integration branch pointing at main, tracking origin/main
        {
            let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("integration", &head_commit, false).unwrap();
            repo.set_head("refs/heads/integration").unwrap();

            // Set upstream tracking
            let mut integration = repo.find_branch("integration", BranchType::Local).unwrap();
            integration.set_upstream(Some("origin/main")).unwrap();
        }

        TestRepo { repo, _dir: dir }
    }

    /// Get the signature used for commits.
    fn sig() -> Signature<'static> {
        Signature::now("Test", "test@test.com").unwrap()
    }

    /// Create a commit with a file.
    ///
    /// # Arguments
    /// * `message` - The commit message
    /// * `filename` - The filename to create/modify
    ///
    /// # Returns
    /// The OID of the created commit
    pub fn commit(&self, message: &str, filename: &str) -> git2::Oid {
        let path = self.repo.workdir().unwrap().join(filename);
        fs::write(&path, message).unwrap();

        let mut index = self.repo.index().unwrap();
        index.add_path(Path::new(filename)).unwrap();
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

    /// Create a commit without changing files (using current tree).
    ///
    /// # Arguments
    /// * `message` - The commit message
    ///
    /// # Returns
    /// The OID of the created commit
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
    ///
    /// # Arguments
    /// * `message` - The commit message
    /// * `parent1_oid` - OID of the first parent
    /// * `parent2_oid` - OID of the second parent
    ///
    /// # Returns
    /// The OID of the merge commit
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

    /// Get a commit relative to HEAD.
    ///
    /// # Arguments
    /// * `steps_back` - Number of steps back from HEAD (0 = HEAD, 1 = HEAD~1, etc.)
    ///
    /// # Returns
    /// The commit at the specified position
    ///
    /// # Example
    /// ```ignore
    /// let head = test_repo.get_commit(0);      // HEAD
    /// let parent = test_repo.get_commit(1);    // HEAD~1
    /// let grandparent = test_repo.get_commit(2); // HEAD~2
    /// ```
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

    /// Get the OID of a commit relative to HEAD.
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn current_branch_name(&self) -> String {
        self.repo.head().unwrap().shorthand().unwrap().to_string()
    }

    /// Check if a branch exists.
    pub fn branch_exists(&self, name: &str) -> bool {
        self.repo.find_branch(name, BranchType::Local).is_ok()
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
    ///
    /// # Arguments
    /// * `name` - The branch name
    /// * `oid` - The commit OID where the branch should point
    pub fn create_branch_at_commit(&self, name: &str, oid: git2::Oid) -> git2::Branch<'_> {
        let commit = self.find_commit(oid);
        self.repo.branch(name, &commit, false).unwrap()
    }

    /// Get the target OID of a remote branch.
    ///
    /// # Arguments
    /// * `name` - The remote branch name (e.g., "origin/main")
    ///
    /// # Returns
    /// The OID that the remote branch points to
    pub fn find_remote_branch_target(&self, name: &str) -> git2::Oid {
        self.repo
            .find_branch(name, BranchType::Remote)
            .unwrap()
            .get()
            .target()
            .unwrap()
    }

    /// Get the target OID of a branch.
    ///
    /// # Arguments
    /// * `name` - The branch name
    ///
    /// # Returns
    /// The OID that the branch points to
    ///
    /// # Panics
    /// Panics if the branch doesn't exist
    pub fn get_branch_target(&self, name: &str) -> git2::Oid {
        self.repo
            .find_branch(name, BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap()
    }

    /// Set HEAD to a detached state at a specific commit.
    ///
    /// # Arguments
    /// * `oid` - The commit OID to detach HEAD to
    pub fn set_detached_head(&self, oid: git2::Oid) {
        self.repo.set_head_detached(oid).unwrap();
    }

    /// Set up a fake editor that replaces commit messages.
    ///
    /// # Arguments
    /// * `new_message` - The message that the fake editor will write
    ///
    /// # Returns
    /// The path to the editor script (for reference)
    #[allow(dead_code)]
    pub fn set_fake_editor(&self, new_message: &str) -> String {
        // Git on Windows uses Git Bash, so we use the same shell command format for all platforms
        let editor_script = format!("sh -c 'echo \"{}\" > \"$1\"' --", new_message);

        // SAFETY: This is a test environment and we're setting a git-specific env var
        // that won't affect other tests or the system
        unsafe {
            std::env::set_var("GIT_EDITOR", &editor_script);
        }

        editor_script
    }

    /// Get the path to the remote repository (if created with new_with_remote).
    ///
    /// Returns None if the repository doesn't have a remote.git setup.
    pub fn remote_path(&self) -> Option<PathBuf> {
        let remote_path = self._dir.path().join("remote.git");
        if remote_path.exists() {
            Some(remote_path)
        } else {
            None
        }
    }

    /// Add commits directly to the remote repository.
    ///
    /// This is useful for simulating upstream changes.
    ///
    /// # Arguments
    /// * `messages` - Commit messages to add to the remote's main branch
    ///
    /// # Returns
    /// OID of the last commit added
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

    /// Fetch from the remote repository.
    ///
    /// Updates origin/* references in the working repository.
    pub fn fetch_remote(&self) {
        self.repo
            .find_remote("origin")
            .unwrap()
            .fetch(&["main"], None, None)
            .unwrap();
    }
}

/// Builder for creating test repositories with a fluent API.
///
/// # Example
/// ```ignore
/// let test_repo = TestRepoBuilder::new()
///     .commit("First commit", "file1.txt")
///     .commit("Second commit", "file2.txt")
///     .branch("feature")
///     .build();
/// ```
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

        // Verify branch was created
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
