use anyhow::{Result, bail};

pub fn run(shell: String) -> Result<()> {
    let script = match shell.as_str() {
        "powershell" | "pwsh" => include_str!("git-loom.ps1"),
        "clink" | "cmd" => include_str!("git-loom.lua"),
        "bash" => include_str!("git-loom.bash"),
        "zsh" => include_str!("git-loom.zsh"),
        "fish" => include_str!("git-loom.fish"),
        _ => bail!(
            "Unsupported shell: '{}'. Supported shells: powershell, clink, bash, zsh, fish",
            shell
        ),
    };
    print!("{script}");
    Ok(())
}
