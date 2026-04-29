//! Webview login — launches Python script that captures cookie via CDP.
//! Full automation: user only logs in, cookie is captured automatically.

use std::env;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

/// Embedded Python script (compiled into the exe).
const WEBVIEW_HOST_SCRIPT: &str = include_str!("../../../webview_host.py");

/// Message type for webview login result.
pub enum WebViewLoginMsg {
    Cookie(String),
    Error(String),
}

/// Spawns Python script and returns a receiver for the result.
pub fn spawn_webview_login() -> mpsc::Receiver<WebViewLoginMsg> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        // Find Python executable
        let python = match find_python() {
            Some(p) => p,
            None => {
                let _ = tx.send(WebViewLoginMsg::Error(
                    "Python not found. Please install Python 3.".to_string(),
                ));
                return;
            }
        };

        // Write embedded script to a temp file
        let script_path = match write_script_to_temp() {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(WebViewLoginMsg::Error(format!(
                    "Failed to write Python script: {}",
                    e
                )));
                return;
            }
        };

        // Use forward slashes for Python
        let script_str = script_path.to_string_lossy().replace("\\", "/");

        // Start Python script
        let mut child = match Command::new(&python)
            .arg(&script_str)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(WebViewLoginMsg::Error(format!(
                    "Failed to start Python script: {}",
                    e
                )));
                let _ = std::fs::remove_file(&script_path);
                return;
            }
        };

        let start = SystemTime::now();
        let timeout = Duration::from_secs(120);

        // Read stdout in a separate thread
        if let Some(mut stdout) = child.stdout.take() {
            let tx_clone = tx.clone();
            thread::spawn(move || {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(&mut stdout);
                for line in reader.lines().flatten() {
                    if !line.trim().is_empty() {
                        let _ = tx_clone.send(WebViewLoginMsg::Cookie(line.trim().to_string()));
                        break;
                    }
                }
            });
        }

        // Wait for cookie or timeout
        while start.elapsed().unwrap_or_default() < timeout {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Process exited, read stderr for errors
                    let mut stderr = String::new();
                    if let Some(mut serr) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut serr, &mut stderr);
                    }
                    if !stderr.is_empty() {
                        let _ = tx.send(WebViewLoginMsg::Error(stderr));
                    }
                    break;
                }
                Ok(None) => {}
                Err(_) => break,
            }
            thread::sleep(Duration::from_millis(500));
        }

        let _ = child.kill();
        // Clean up temp file
        let _ = std::fs::remove_file(&script_path);
    });

    rx
}

/// Write the embedded Python script to a temporary file.
fn write_script_to_temp() -> std::io::Result<std::path::PathBuf> {
    let mut temp_dir = env::temp_dir();
    temp_dir.push("webview_host.py");

    let mut file = std::fs::File::create(&temp_dir)?;
    file.write_all(WEBVIEW_HOST_SCRIPT.as_bytes())?;
    file.flush()?;

    Ok(temp_dir)
}

fn find_python() -> Option<std::path::PathBuf> {
    // Try different Python executable names
    for name in &["python3.exe", "python.exe", "py.exe"] {
        if let Ok(output) = Command::new("where").arg(name).output() {
            if output.status.success() {
                if let Ok(path_str) = String::from_utf8(output.stdout) {
                    // Take only the first line (where can return multiple paths)
                    let first_path = path_str.lines().next().unwrap_or("").trim();
                    if !first_path.is_empty() && std::path::Path::new(first_path).exists() {
                        return Some(std::path::PathBuf::from(first_path));
                    }
                }
            }
        }
    }
    None
}
