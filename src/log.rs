use crate::{git, graph};
use git2::Repository;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover(".")?;
    let info = git::gather_repo_info(&repo)?;
    let output = graph::render(info);
    print!("{}", output);
    Ok(())
}
