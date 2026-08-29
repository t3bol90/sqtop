//! Desktop notification helper for sqtop.
//!
//! This module provides desktop notifications via platform-specific commands.
//! Failures are silent - terminal bell is the primary signal.

use std::process::Command;

/// Fire a desktop notification. Silent failure.
pub fn desktop_notify(title: &str, message: &str) {
    // Only attempt if desktop notifications are useful (not in SSH)
    if std::env::var("SSH_CONNECTION").is_ok() {
        return;
    }

    let _ = send_notification(title, message);
}

#[cfg(target_os = "macos")]
fn send_notification(title: &str, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    let script = format!(
        r#"display notification "{}" with title "{}""#,
        escape_applescript(message),
        escape_applescript(title)
    );

    Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn send_notification(title: &str, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    Command::new("notify-send")
        .arg("--expire-time=8000")
        .arg(title)
        .arg(message)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn send_notification(_title: &str, _message: &str) -> Result<(), Box<dyn std::error::Error>> {
    // No-op on unsupported platforms
    Ok(())
}

/// Escape a string for embedding in an AppleScript literal.
///
/// macOS-only: this exists solely for the `osascript` path, so it is compiled
/// only there. Without the gate it is dead code on Linux and fails the lint
/// gate, which is how CI caught it.
#[cfg(target_os = "macos")]
fn escape_applescript(s: &str) -> String {
    s.replace('\\', r#"\\"#).replace('"', r#"\""#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn test_escape_applescript_quotes() {
        assert_eq!(escape_applescript(r#"hello "world""#), r#"hello \"world\""#);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_escape_applescript_backslashes() {
        assert_eq!(escape_applescript(r#"path\to\file"#), r#"path\\to\\file"#);
    }

    #[test]
    fn test_desktop_notify_does_not_panic() {
        // Should not panic, even if notification fails
        desktop_notify("Test", "This is a test");
    }
}
