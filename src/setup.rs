use std::path::PathBuf;

const SYSTEMD_SERVICE: &str = r#"[Unit]
Description=gh-triage GitHub notification daemon
After=network-online.target

[Service]
ExecStart=%h/.cargo/bin/gh-triage daemon
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
"#;

fn waybar_config(terminal: &str) -> String {
    format!(
        r#""custom/gh-triage": {{
    "exec": "gh-triage waybar",
    "interval": 60,
    "return-type": "json",
    "on-click": "{terminal} -e gh-triage"
}}"#
    )
}

fn systemd_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/systemd/user")
}

fn waybar_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("waybar")
}

pub fn install_systemd() -> Result<(), crate::AppError> {
    let dir = systemd_dir();
    std::fs::create_dir_all(&dir).map_err(|e| crate::AppError::Setup(e.to_string()))?;

    let path = dir.join("gh-triage.service");
    if path.exists() {
        println!("Service file already exists at {}", path.display());
        println!("Remove it first if you want to regenerate.");
        return Ok(());
    }

    std::fs::write(&path, SYSTEMD_SERVICE).map_err(|e| crate::AppError::Setup(e.to_string()))?;

    println!("Wrote {}", path.display());
    println!();
    println!("Enable and start with:");
    println!("  systemctl --user enable --now gh-triage");
    println!();
    println!("Check status with:");
    println!("  systemctl --user status gh-triage");

    Ok(())
}

pub fn install_waybar(terminal: Option<String>) -> Result<(), crate::AppError> {
    let terminal = terminal.unwrap_or_else(|| "alacritty".to_string());
    let snippet = waybar_config(&terminal);

    let waybar_config_path = waybar_dir().join("config");
    let waybar_jsonc_path = waybar_dir().join("config.jsonc");

    let hint = if waybar_jsonc_path.exists() {
        waybar_jsonc_path.display().to_string()
    } else if waybar_config_path.exists() {
        waybar_config_path.display().to_string()
    } else {
        "your Waybar config file".to_string()
    };

    println!("Add the following to {hint}:");
    println!();
    println!("{snippet}");
    println!();
    println!("Then reload Waybar:");
    println!("  killall -SIGUSR2 waybar");

    Ok(())
}
