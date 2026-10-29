//! development checks for the Aegis workspace
#![forbid(unsafe_code)]

use std::env;
use std::fmt;
use std::process::{Command, ExitCode};

#[derive(Debug)]
enum Error {
    Start(std::io::Error),
    Failed(&'static str),
    MissingTool,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(error) => write!(f, "could not start cargo: {error}"),
            Self::Failed(step) => write!(f, "{step} failed"),
            Self::MissingTool => write!(
                f,
                "missing cargo-audit or cargo-deny; run cargo install --locked cargo-audit cargo-deny"
            ),
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args != ["ci"] {
        eprintln!("usage: cargo xtask ci");
        return ExitCode::FAILURE;
    }

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    match ci(&cargo) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn ci(cargo: &str) -> Result<(), Error> {
    run(cargo, "fmt", &["fmt", "--all", "--check"])?;
    run(
        cargo,
        "clippy",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(cargo, "build", &["build", "--workspace"])?;
    run(cargo, "test", &["test", "--workspace"])?;
    check_tool(cargo, "audit")?;
    run(cargo, "audit", &["audit"])?;
    check_tool(cargo, "deny")?;
    run(cargo, "deny", &["deny", "check"])?;
    Ok(())
}

fn check_tool(cargo: &str, tool: &str) -> Result<(), Error> {
    let status = Command::new(cargo)
        .args([tool, "--version"])
        .status()
        .map_err(Error::Start)?;
    if status.success() {
        return Ok(());
    }
    Err(Error::MissingTool)
}

fn run(cargo: &str, name: &'static str, args: &[&str]) -> Result<(), Error> {
    println!("{name}");
    let status = Command::new(cargo)
        .args(args)
        .status()
        .map_err(Error::Start)?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Failed(name))
}
