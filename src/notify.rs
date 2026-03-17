use std::process::Command;

/// Send a desktop notification via notify-send. Logs and continues if not available.
pub fn send_notification(urgency: &str, repo: &str, title: &str) {
    let result = Command::new("notify-send")
        .args(["-u", urgency, &format!("GitHub: {repo}"), title])
        .output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                eprintln!(
                    "notify-send failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        Err(e) => {
            eprintln!("notify-send not found or failed to run: {e}");
        }
    }
}
