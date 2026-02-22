use anyhow::{Result, bail};

pub fn run(shell: String) -> Result<()> {
    match shell.as_str() {
        "powershell" | "pwsh" => {
            print!("{}", include_str!("git-loom.ps1"));
            Ok(())
        }
        "clink" | "cmd" => {
            print!("{}", include_str!("git-loom.lua"));
            Ok(())
        }
        _ => bail!(
            "Unsupported shell: '{}'. Supported shells: powershell, clink",
            shell
        ),
    }
}
