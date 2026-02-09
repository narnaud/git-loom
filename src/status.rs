use crate::{git, graph};
use git2::Repository;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let repo = Repository::discover(cwd)?;
    let info = git::gather_repo_info(&repo)?;
    let output = graph::render(info);
    print!("{}", output);
    Ok(())
}
