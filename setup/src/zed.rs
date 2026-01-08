//! Zed editor configuration

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Configure Zed with Symposium agent using act-as-configured mode
pub fn configure_zed(symposium_acp_agent_path: &Path, dry_run: bool) -> Result<()> {
    let zed_config_path = get_zed_config_path()?;

    if !zed_config_path.exists() {
        println!("⚠️  Zed settings.json not found, skipping Zed configuration");
        println!("   Expected path: {}", zed_config_path.display());
        return Ok(());
    }

    println!("🔧 Configuring Zed editor...");
    println!("   Config file: {}", zed_config_path.display());

    // Read existing configuration
    let contents =
        std::fs::read_to_string(&zed_config_path).context("Failed to read Zed settings.json")?;

    // Parse JSON5 (supports comments and trailing commas)
    let mut config: Value =
        json5::from_str(&contents).context("Failed to parse Zed settings.json")?;

    // Ensure agent_servers map exists
    if !config.get("agent_servers").is_some() {
        config["agent_servers"] = json!({});
    }

    let agent_servers = config["agent_servers"]
        .as_object_mut()
        .context("agent_servers is not an object")?;

    // Create single Symposium agent using act-as-configured
    let agent_config = json!({
        "type": "custom",
        "command": symposium_acp_agent_path.to_string_lossy(),
        "args": ["act-as-configured"],
        "env": {}
    });

    if dry_run {
        println!("   Would add configuration for: Symposium");
        println!(
            "   Config: {}",
            serde_json::to_string_pretty(&agent_config).unwrap()
        );
    } else {
        println!("   Adding configuration for: Symposium");
        agent_servers.insert("Symposium".to_string(), agent_config);

        // Write back configuration
        let formatted =
            serde_json::to_string_pretty(&config).context("Failed to serialize config")?;

        std::fs::write(&zed_config_path, formatted).context("Failed to write Zed settings.json")?;

        println!("✅ Zed configuration updated");
        println!("   On first use, Symposium will prompt you to select an agent");
    }

    Ok(())
}

/// Get the path to Zed settings.json
fn get_zed_config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".config/zed/settings.json"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_trailing_comma() {
        let input = r#"{"key": "value",}"#;
        let result: Result<serde_json::Value, _> = json5::from_str(input);
        println!("Result: {:?}", result);
        assert!(result.is_ok(), "json5 should handle trailing commas");
    }
}
