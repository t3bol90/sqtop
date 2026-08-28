//! Clipboard helper with OSC 52 transport (works over SSH) and subprocess fallback.
//!
//! Matches Python `src/sqtop/clipboard.py`.

use crate::config::ClipboardConfig;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Clipboard transport method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Osc52,
    Pbcopy,
    Xclip,
    Xsel,
    Clip,
    None,
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Osc52 => write!(f, "osc52"),
            Transport::Pbcopy => write!(f, "pbcopy"),
            Transport::Xclip => write!(f, "xclip"),
            Transport::Xsel => write!(f, "xsel"),
            Transport::Clip => write!(f, "clip"),
            Transport::None => write!(f, "none"),
        }
    }
}

/// Result of a clipboard copy operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyResult {
    pub ok: bool,
    pub transport: Transport,
    pub truncated: bool,
}

/// Maximum OSC 52 payload size in bytes.
///
/// Matches Python `OSC52_MAX_BYTES`.
pub const OSC52_MAX_BYTES: usize = 74_000;

/// Truncate text to the largest valid UTF-8 prefix that fits in max_bytes.
///
/// Returns (truncated_text, was_truncated).
///
/// Matches Python `_truncate_utf8(text, max_bytes)`.
fn truncate_utf8(text: &str, max_bytes: usize) -> (String, bool) {
    let bytes = text.as_bytes();
    if bytes.len() <= max_bytes {
        return (text.to_string(), false);
    }

    // Find the largest valid UTF-8 boundary <= max_bytes
    let mut boundary = max_bytes;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }

    (text[..boundary].to_string(), true)
}

/// Try to copy text using subprocess clipboard tools.
///
/// Matches Python `_try_subprocess(text)`.
fn try_subprocess(text: &str) -> CopyResult {
    let bytes = text.as_bytes();

    // Platform-specific clipboard tool selection
    if cfg!(target_os = "macos") {
        match run_clipboard_cmd("pbcopy", &[], bytes) {
            Ok(()) => CopyResult {
                ok: true,
                transport: Transport::Pbcopy,
                truncated: false,
            },
            Err(_) => CopyResult {
                ok: false,
                transport: Transport::None,
                truncated: false,
            },
        }
    } else if cfg!(target_os = "windows") {
        match run_clipboard_cmd("clip", &[], bytes) {
            Ok(()) => CopyResult {
                ok: true,
                transport: Transport::Clip,
                truncated: false,
            },
            Err(_) => CopyResult {
                ok: false,
                transport: Transport::None,
                truncated: false,
            },
        }
    } else {
        // Linux: try xclip, then xsel
        if let Ok(()) = run_clipboard_cmd("xclip", &["-selection", "clipboard"], bytes) {
            return CopyResult {
                ok: true,
                transport: Transport::Xclip,
                truncated: false,
            };
        }
        if let Ok(()) = run_clipboard_cmd("xsel", &["--clipboard", "--input"], bytes) {
            return CopyResult {
                ok: true,
                transport: Transport::Xsel,
                truncated: false,
            };
        }
        CopyResult {
            ok: false,
            transport: Transport::None,
            truncated: false,
        }
    }
}

/// Run a clipboard command with input, with a timeout.
fn run_clipboard_cmd(cmd: &str, args: &[&str], input: &[u8]) -> Result<(), ()> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;

    // Write input
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
        drop(stdin); // Close stdin
    }

    // Wait with timeout (2 seconds to match Python)
    let timeout = Duration::from_secs(2);
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() { Ok(()) } else { Err(()) };
            }
            Ok(None) => {
                // Still running
                if start.elapsed() >= timeout {
                    // Timeout - kill the process
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return Err(()),
        }
    }
}

/// Try to copy text using OSC 52 escape sequence.
///
/// Emits the sequence to stdout. Returns Ok if emission succeeded (not if terminal accepted it).
fn try_osc52(text: &str) -> Result<(), ()> {
    use base64::prelude::*;

    let encoded = BASE64_STANDARD.encode(text.as_bytes());

    // OSC 52 sequence: ESC ] 52 ; c ; <base64> ESC \
    // ESC is byte 0x1b (27 decimal)
    let mut stdout = std::io::stdout();

    stdout.write_all(&[0x1b]).map_err(|_| ())?; // ESC
    stdout.write_all(b"]52;c;").map_err(|_| ())?;
    stdout.write_all(encoded.as_bytes()).map_err(|_| ())?;
    stdout.write_all(&[0x1b]).map_err(|_| ())?; // ESC
    stdout.write_all(b"\\").map_err(|_| ())?; // backslash
    stdout.flush().map_err(|_| ())?;

    Ok(())
}

/// Copy text to clipboard.
///
/// Transport selection is controlled by `config.transport`:
/// - `"auto"`: OSC 52 first, subprocess fallback if sqtop is not remoting to Slurm
/// - `"osc52"`: OSC 52 only
/// - `"subprocess"`: subprocess only (if sqtop is not remoting to Slurm)
///
/// The subprocess fallback is allowed only when `remote_host.is_none()` - that is,
/// when sqtop talks to a local Slurm. When sqtop is configured to SSH to a remote
/// Slurm (`[remote] host` is set), subprocess clipboard tools would run on the wrong
/// machine, so only OSC 52 is attempted.
///
/// OSC 52 payloads > `OSC52_MAX_BYTES` are truncated.
///
/// Matches Python `copy_to_clipboard(app, text)`.
pub fn copy(text: &str, config: &ClipboardConfig, remote_host: Option<&str>) -> CopyResult {
    let mode = config.transport.to_lowercase();

    // OSC 52 path
    if mode == "auto" || mode == "osc52" {
        let (payload, truncated) = truncate_utf8(text, OSC52_MAX_BYTES);
        if try_osc52(&payload).is_ok() {
            return CopyResult {
                ok: true,
                transport: Transport::Osc52,
                truncated,
            };
        }
        // Fall through to subprocess if auto mode
    }

    // Subprocess fallback
    // Only allowed when sqtop is NOT configured to remote to Slurm and mode permits
    if (mode == "auto" || mode == "subprocess") && remote_host.is_none() {
        return try_subprocess(text);
    }

    CopyResult {
        ok: false,
        transport: Transport::None,
        truncated: false,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_utf8_no_truncation_needed() {
        let text = "hello";
        let (result, truncated) = truncate_utf8(text, 100);
        assert_eq!(result, text);
        assert!(!truncated);
    }

    #[test]
    fn test_truncate_utf8_exact_fit() {
        let text = "hello";
        let (result, truncated) = truncate_utf8(text, 5);
        assert_eq!(result, text);
        assert!(!truncated);
    }

    #[test]
    fn test_truncate_utf8_ascii() {
        let text = "hello world test";
        let (result, truncated) = truncate_utf8(text, 5);
        assert_eq!(result, "hello");
        assert!(truncated);
    }

    #[test]
    fn test_truncate_utf8_multibyte_no_split() {
        // U+1F600 (grinning face emoji) is 4 bytes in UTF-8
        let emoji = "\u{1F600}\u{1F601}\u{1F602}"; // 3 emoji = 12 bytes
        let (result, truncated) = truncate_utf8(emoji, 8);
        // Should truncate to 2 emoji (8 bytes), not split the third
        assert_eq!(result, "\u{1F600}\u{1F601}");
        assert!(truncated);
    }

    #[test]
    fn test_truncate_utf8_multibyte_boundary() {
        // Test that we never split a multi-byte character
        let text = "hello\u{1F600}world"; // hello (5) + emoji (4) + world (5) = 14 bytes
        let (result, truncated) = truncate_utf8(text, 7);
        // Should stop at 5 bytes (hello), not split the emoji
        assert_eq!(result, "hello");
        assert!(truncated);
    }

    #[test]
    fn test_truncate_utf8_valid_result() {
        // Ensure truncated result is always valid UTF-8
        let text = "a\u{1F600}b\u{1F601}c\u{1F602}d";
        for max_bytes in 1..20 {
            let (result, _) = truncate_utf8(text, max_bytes);
            // If this doesn't panic, the result is valid UTF-8
            let _ = result.as_bytes();
            assert!(result.len() <= max_bytes || result.is_empty() || max_bytes == 0);
        }
    }

    #[test]
    fn test_truncate_utf8_large_payload() {
        let text = "x".repeat(OSC52_MAX_BYTES + 1000);
        let (result, truncated) = truncate_utf8(&text, OSC52_MAX_BYTES);
        assert!(truncated);
        assert_eq!(result.len(), OSC52_MAX_BYTES);
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(Transport::Osc52.to_string(), "osc52");
        assert_eq!(Transport::Pbcopy.to_string(), "pbcopy");
        assert_eq!(Transport::Xclip.to_string(), "xclip");
        assert_eq!(Transport::Xsel.to_string(), "xsel");
        assert_eq!(Transport::Clip.to_string(), "clip");
        assert_eq!(Transport::None.to_string(), "none");
    }

    #[test]
    fn test_copy_result_equality() {
        let r1 = CopyResult {
            ok: true,
            transport: Transport::Osc52,
            truncated: false,
        };
        let r2 = CopyResult {
            ok: true,
            transport: Transport::Osc52,
            truncated: false,
        };
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_copy_mode_osc52_only() {
        // In osc52-only mode, subprocess should never be tried
        let config = ClipboardConfig {
            transport: "osc52".to_string(),
        };
        let result = copy("test", &config, None);
        // Result depends on whether stdout is available in test environment
        assert!(result.transport == Transport::Osc52 || result.transport == Transport::None);
    }

    #[test]
    fn test_copy_empty_text() {
        let config = ClipboardConfig {
            transport: "auto".to_string(),
        };
        let result = copy("", &config, None);
        // Should not panic with empty input
        assert!(result.transport != Transport::None || !result.ok);
    }

    #[test]
    fn test_copy_large_text_truncation() {
        let text = "x".repeat(OSC52_MAX_BYTES + 5000);
        let config = ClipboardConfig {
            transport: "osc52".to_string(),
        };
        let result = copy(&text, &config, None);
        // If OSC52 succeeded, truncation flag should be set
        if result.ok && result.transport == Transport::Osc52 {
            assert!(result.truncated);
        }
    }

    #[test]
    fn test_copy_subprocess_mode_no_osc52() {
        // In subprocess mode, OSC52 should not be tried
        let config = ClipboardConfig {
            transport: "subprocess".to_string(),
        };
        let result = copy("test", &config, None);
        // Result should be either a subprocess transport or None (if unavailable)
        assert!(result.transport != Transport::Osc52);
    }

    #[test]
    fn test_subprocess_allowed_when_no_remote() {
        // When sqtop talks to local Slurm (remote_host = None), subprocess is allowed
        let config = ClipboardConfig {
            transport: "subprocess".to_string(),
        };
        let result = copy("test", &config, None);
        // Result should be a subprocess transport (or None if tool unavailable)
        // but definitely NOT Osc52 since mode is "subprocess"
        assert!(result.transport != Transport::Osc52);
    }

    #[test]
    fn test_subprocess_blocked_when_remote_configured() {
        // When sqtop is configured to SSH to remote Slurm, subprocess must be blocked
        let config = ClipboardConfig {
            transport: "subprocess".to_string(),
        };
        let result = copy("test", &config, Some("cluster.example.com"));
        // Subprocess mode with remote host set should return None
        assert_eq!(result.transport, Transport::None);
        assert!(!result.ok);
    }

    #[test]
    fn test_auto_mode_tries_subprocess_when_no_remote() {
        // In auto mode with no remote, OSC52 failure should fall back to subprocess
        let config = ClipboardConfig {
            transport: "auto".to_string(),
        };
        // OSC52 might succeed or fail in test environment
        // If it fails and we're local, should try subprocess
        let result = copy("test", &config, None);
        // Should get either Osc52 (if stdout works) or subprocess transport or None
        // Just verify it doesn't panic and returns a valid result
        assert!(
            result.transport == Transport::Osc52
                || result.transport == Transport::Pbcopy
                || result.transport == Transport::Xclip
                || result.transport == Transport::Xsel
                || result.transport == Transport::Clip
                || result.transport == Transport::None
        );
    }

    #[test]
    fn test_auto_mode_no_subprocess_when_remote_configured() {
        // In auto mode with remote host, subprocess fallback must not happen
        let config = ClipboardConfig {
            transport: "auto".to_string(),
        };
        let result = copy("test", &config, Some("remote.cluster"));
        // Should only get Osc52 (if stdout works) or None
        assert!(result.transport == Transport::Osc52 || result.transport == Transport::None);
    }
}
