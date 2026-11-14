use std::path::Path;
use std::process::{Command, Stdio};

/// Upgrade GNUPlex from the git checkout containing the executable.
/// The boolean result says whether an upgrade actually happened.
pub fn upgrade_gnuplex(exe: &str, interactive: bool) -> Result<bool, String> {
    let cwd = Path::new(exe)
        .parent()
        .map(|p| p.join("../.."))
        .unwrap_or_else(|| Path::new("../..").to_path_buf());

    let branch = git_output(&cwd, &["branch", "--show-current"])
        .map_err(|e| format!("19c5e798-666b-4aae-818b-56061645ca9f git problem {e}"))?;
    let branch = String::from_utf8_lossy(&branch).trim().to_string();

    git_status(&cwd, &["fetch", "origin", &branch]).map_err(|e| format!("fetch failed {e}"))?;

    let local = git_output(&cwd, &["rev-parse", &branch]).map_err(|e| e.to_string())?;
    let remote =
        git_output(&cwd, &["rev-parse", &format!("origin/{branch}")]).map_err(|e| e.to_string())?;

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
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("exit status {}", status.code().unwrap_or(-1)));
    }
    if interactive {
        println!("Successfully upgraded! Now run `systemctl --user restart gnuplex`.");
    }
    Ok(true)
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "exit status {}",
            output.status.code().unwrap_or(-1)
        ));
    }
    Ok(output.stdout)
}

fn git_status(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("exit status {}", status.code().unwrap_or(-1)));
    }
    Ok(())
}
