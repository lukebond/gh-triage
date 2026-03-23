use crate::config::SummaryConfig;
use tokio::process::Command;

/// Generate an initial AI summary for a new item.
/// Returns None if no summary command is configured or it fails.
pub async fn generate_summary(
    config: &SummaryConfig,
    item_type: &str,
    title: &str,
    body: &str,
) -> Option<String> {
    let body_truncated: String = body.chars().take(2000).collect();
    let prompt = format!(
        "Summarise this GitHub {} in 1-2 sentences. Be concise and focus on what action (if any) is needed from a reviewer or assignee.\n\nTitle: {}\nBody: {}",
        item_type, title, body_truncated
    );
    run_summary_command(config, &prompt).await
}

/// Regenerate a summary for an updated item, highlighting what's new.
/// `new_comments` contains only the comments added since we last looked.
pub async fn generate_update_summary(
    config: &SummaryConfig,
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
    run_summary_command(config, &prompt).await
}

async fn run_summary_command(config: &SummaryConfig, prompt: &str) -> Option<String> {
    let args: Vec<String> = config
        .args
        .iter()
        .map(|a| a.replace("{prompt}", prompt))
        .collect();

    let result = Command::new(&config.command).args(&args).output().await;

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
