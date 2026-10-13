//! Offline stand-in for the GitHub CLI, for the git-loom demo video only.
//! Answers the handful of calls `loom push` makes; never touches the network.
use std::{env, fs, process::exit};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let flag = |name: &str| {
        a.iter()
            .position(|x| *x == name)
            .and_then(|i| a.get(i + 1).copied())
            .unwrap_or("")
    };
    match a.as_slice() {
        ["--version", ..] => println!("gh version 2.74.0 (demo stub)"),
        ["pr", "list", ..] => println!("[]"),
        ["pr", "create", ..] => {
            let state = env::var("FAKE_GH_STATE").unwrap_or_else(|_| "fake-gh-state".into());
            let n: u64 = fs::read_to_string(&state)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(41)
                + 1;
            let _ = fs::write(&state, n.to_string());
            println!("https://github.com/{}/pull/{}", flag("--repo"), n);
        }
        ["pr", "edit", ..] => {}
        ["api", "--method", "POST", ..] => println!("{{\"number\":7}}"),
        ["api", ..] => println!("null"),
        _ => {
            eprintln!("fake gh: unsupported call {:?}", a);
            exit(1);
        }
    }
}
