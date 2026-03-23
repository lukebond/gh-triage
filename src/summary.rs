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

/// Generate an initial AI summary for a new item.
/// Returns None if `claude` is not in PATH or fails.
pub async fn generate_summary(item_type: &str, title: &str, body: &str) -> Option<String> {
    let body_truncated: String = body.chars().take(2000).collect();
    let prompt = format!(
        "Summarise this GitHub {} in 1-2 sentences. Be concise and focus on what action (if any) is needed from a reviewer or assignee.\n\nTitle: {}\nBody: {}",
        item_type, title, body_truncated
    );
    run_claude(&prompt).await
}

/// Regenerate a summary for an updated item, highlighting what's new.
/// `new_comments` contains only the comments added since we last looked.
pub async fn generate_update_summary(
    item_type: &str,
    title: &str,
    body: &str,
    new_comments: &[(String, String)], // (author, body)
) -> Option<String> {
    let body_truncated: String = body.chars().take(1000).collect();
    let comments_text: String = new_comments
        .iter()
        .map(|(author, comment)| {
            let truncated: String = comment.chars().take(500).collect();
            format!("@{author}: {truncated}")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let prompt = format!(
        "Summarise this GitHub {} in 2-3 sentences. First sentence: a brief reminder of what this is about. \
         Remaining sentences: highlight what's new based on the recent comments below. \
         Don't list every comment — just capture the key developments.\n\n\
         Title: {}\n\
         Original description: {}\n\n\
         New comments since last check:\n{}",
        item_type, title, body_truncated, comments_text
    );
    run_claude(&prompt).await
}

async fn run_claude(prompt: &str) -> Option<String> {
    let result = Command::new("claude")
        .args([
            "-p",
            prompt,
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
