use thiserror::Error;
use tokio::process::Command;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum SummaryError {
    #[error("failed to run claude: {0}")]
    SpawnError(#[from] std::io::Error),
    #[error("claude exited with non-zero status")]
    NonZeroExit,
}

/// Generate an AI summary by shelling out to `claude`.
/// Returns None if `claude` is not in PATH or fails.
pub async fn generate_summary(item_type: &str, title: &str, body: &str) -> Option<String> {
    let body_truncated: String = body.chars().take(2000).collect();
    let prompt = format!(
        "Summarise this GitHub {} in 1-2 sentences. Be concise and focus on what action (if any) is needed from a reviewer or assignee.\n\nTitle: {}\nBody: {}",
        item_type, title, body_truncated
    );

    let result = Command::new("claude")
        .args([
            "-p",
            &prompt,
            "--no-session-persistence",
            "--allowedTools",
            "",
            "--output-format",
            "text",
        ])
        .output()
        .await;

    match result {
        Ok(output) => {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            } else {
                None
            }
        }
        Err(_) => None,
    }
}
