//! Repository automation: `cargo xtask <task>`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1);
    let root = project_root();
    let result = match task.as_deref() {
        Some("build-dashboard") => build_dashboard(&root),
        Some("ci") => ci(&root),
        _ => {
            eprintln!("usage: cargo xtask <build-dashboard|ci>");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in <root>/xtask")
        .to_path_buf()
}

/// Builds `web/dashboard` with npm and copies the bundle into `crates/trunks-dashboard/dist`.
fn build_dashboard(root: &Path) -> Result<(), String> {
    let web = root.join("web/app");
    let out = root.join("crates/trunks-dashboard/dist");
    if !web.join("node_modules").exists() {
        run("npm", &["ci"], &web)?;
    }
    run("npm", &["run", "build"], &web)?;
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| e.to_string())?;
    }
    copy_dir(&web.join("dist"), &out).map_err(|e| e.to_string())?;
    std::fs::write(out.join(".gitkeep"), b"").map_err(|e| e.to_string())?;
    println!("dashboard bundle copied to {}", out.display());
    Ok(())
}

/// Runs the same checks as CI.
fn ci(root: &Path) -> Result<(), String> {
    run("cargo", &["fmt", "--all", "--check"], root)?;
    run("cargo", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], root)?;
    run("cargo", &["test", "--workspace"], root)?;
    Ok(())
}

fn run(cmd: &str, args: &[&str], cwd: &Path) -> Result<(), String> {
    println!("$ {cmd} {}", args.join(" "));
    let status = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .status()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} {} failed with {status}", args.join(" ")))
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
