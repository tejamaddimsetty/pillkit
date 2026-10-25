//! `barkit-host install` copies the running binary into the per-user data dir
//! and registers it to start at login. No admin rights are needed anywhere.

use std::path::PathBuf;

use crate::paths::{data_dir, log};

pub fn installed_path() -> PathBuf {
    let name = if cfg!(windows) {
        "barkit-host.exe"
    } else {
        "barkit-host"
    };
    data_dir().join("bin").join(name)
}

#[cfg(target_os = "macos")]
fn agent_plist() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Library/LaunchAgents/dev.barkit.host.plist")
}

pub fn install() -> std::io::Result<PathBuf> {
    let target = installed_path();
    std::fs::create_dir_all(target.parent().expect("has parent"))?;
    let me = std::env::current_exe()?;
    if me != target {
        // Copy to a temp name first so a running host is never left with half a binary.
        let tmp = target.with_extension("new");
        std::fs::copy(&me, &tmp)?;
        std::fs::rename(&tmp, &target)?;
    }

    #[cfg(target_os = "macos")]
    {
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.barkit.host</string>
  <key>ProgramArguments</key><array><string>{}</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
            target.display()
        );
        let path = agent_plist();
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(&path, plist)?;
    }

    #[cfg(windows)]
    {
        let value = format!("\"{}\" run", target.display());
        let status = std::process::Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "barkit",
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ])
            .status()?;
        if !status.success() {
            return Err(std::io::Error::other("could not add the login entry"));
        }
    }

    log(format!("installed to {}", target.display()));
    Ok(target)
}

pub fn uninstall() -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = std::fs::remove_file(agent_plist());
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "barkit",
                "/f",
            ])
            .status();
    }
    let _ = std::fs::remove_dir_all(data_dir().join("apps"));
    let _ = std::fs::remove_file(installed_path());
    log("uninstalled");
    Ok(())
}
