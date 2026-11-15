use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Upgrade GNUPlex from the git checkout containing the executable.
/// The boolean result says whether an upgrade actually happened.
pub fn upgrade_gnuplex(exe: &Path, interactive: bool) -> Result<bool> {
    let cwd = exe
        .parent()
        .map(|parent| parent.join("../.."))
        .unwrap_or_else(|| Path::new("../..").to_path_buf());

    let branch = git_output(&cwd, &["branch", "--show-current"])
        .map_err(|err| Error::Git(format!("19c5e798-666b-4aae-818b-56061645ca9f {err}")))?;
    let branch = String::from_utf8_lossy(&branch).trim().to_string();

    git_status(&cwd, &["fetch", "origin", &branch])?;

    let local = git_output(&cwd, &["rev-parse", &branch])?;
    let remote = git_output(&cwd, &["rev-parse", &format!("origin/{branch}")])?;

    if local == remote {
        if interactive {
            println!("Nothing to upgrade");
        }
        return Ok(false);
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(&cwd)
        .args(["pull", "origin", &branch])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        return Err(Error::Git(format!(
            "git pull exited with status {}",
            status.code().unwrap_or(-1)
        )));
    }
    if interactive {
        println!("Successfully upgraded! Now run `systemctl --user restart gnuplex`.");
    }
    Ok(true)
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(Error::Git(format!(
            "git {} exited with status {}",
            args.first().copied().unwrap_or_default(),
            output.status.code().unwrap_or(-1)
        )));
    }
    Ok(output.stdout)
}

fn git_status(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Error::Git(format!(
            "git {} exited with status {}",
            args.first().copied().unwrap_or_default(),
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}
