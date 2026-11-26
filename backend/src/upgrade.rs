use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use crate::error::{Error, Result};

/// Upgrade GNUPlex from the git checkout containing the executable.
/// The boolean result says whether an upgrade actually happened.
pub fn upgrade_gnuplex(exe: &Path, interactive: bool) -> Result<bool> {
    let cwd = exe.parent().map_or_else(
        || Path::new("../..").to_path_buf(),
        |parent| parent.join("../.."),
    );

    let branch = git_output(&cwd, &["branch", "--show-current"])
        .map_err(|err| Error::Git(format!("19c5e798-666b-4aae-818b-56061645ca9f: {err}")))?;
    let branch = String::from_utf8_lossy(&branch).trim().to_string();

    git_output(&cwd, &["fetch", "origin", &branch])?;

    let local = git_output(&cwd, &["rev-parse", &branch])?;
    let remote = git_output(&cwd, &["rev-parse", &format!("origin/{branch}")])?;

    if local == remote {
        if interactive {
            println!("Nothing to upgrade");
        }
        return Ok(false);
    }

    git_inherit(&cwd, &["pull", "origin", &branch])?;
    if interactive {
        println!("Successfully upgraded! Now run `systemctl --user restart gnuplex`.");
    }
    Ok(true)
}

fn git_command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args(args).stdin(Stdio::null());
    command
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = git_command(cwd, args).output()?;
    ensure_success(args, output.status, &output.stderr)?;
    Ok(output.stdout)
}

fn git_inherit(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = git_command(cwd, args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    ensure_success(args, status, "")
}

fn ensure_success(args: &[&str], status: ExitStatus, stderr: impl AsRef<[u8]>) -> Result<()> {
    if status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(stderr.as_ref());
    let stderr = stderr.trim();
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    };
    Err(Error::Git(format!(
        "git {} exited with status {}{detail}",
        args.join(" "),
        status.code().unwrap_or(-1)
    )))
}
