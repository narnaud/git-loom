use crate::{git, graph};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let repo = git::open_repo()?;
    let info = git::gather_repo_info(&repo)?;
    let output = graph::render(info);
    print!("{}", output);
    Ok(())
}
