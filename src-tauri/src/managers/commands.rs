use crate::settings::VoiceCommand;
use log::{debug, error, info};
use tauri::AppHandle;

/// Check if transcribed text is a voice command.
/// Returns the matched command if the text (after stripping wake phrase)
/// matches an enabled command phrase (case-insensitive, substring match).
pub fn parse_command(text: &str, commands: &[VoiceCommand], wake_phrase: &str) -> Option<VoiceCommand> {
    let trimmed = text.trim();
    
    // Strip wake phrase prefix (e.g. "hey jarvis")
    let after_wake = if trimmed.to_lowercase().starts_with(wake_phrase) {
        trimmed[wake_phrase.len()..].trim()
    } else {
        trimmed
    };

    if after_wake.is_empty() {
        return None;
    }

    for cmd in commands {
        if !cmd.enabled {
            continue;
        }
        let phrase_lower = cmd.phrase.to_lowercase();
        let text_lower = after_wake.to_lowercase();
        // Substring match: "turn on youtube" matches "turn on youtube"
        // or even if extra words follow
        if text_lower.contains(&phrase_lower) {
            debug!("Voice command matched: '{}' → '{}'", after_wake, cmd.phrase);
            return Some(cmd.clone());
        }
    }
    None
}

/// Execute a voice command action.
pub fn execute_command(app: &AppHandle, cmd: &VoiceCommand) -> Result<(), String> {
    match cmd.action_type.as_str() {
        "open_url" => {
            let url = cmd.action_payload.trim();
            if url.is_empty() {
                return Err("Empty URL payload".to_string());
            }
            let url = if !url.starts_with("http://") && !url.starts_with("https://") {
                format!("https://{}", url)
            } else {
                url.to_string()
            };
            info!("Executing voice command: open_url({})", url);
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd")
                    .args(["/c", "start", "", &url])
                    .spawn()
                    .map_err(|e| format!("Failed to open URL: {}", e))?;
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open")
                    .arg(&url)
                    .spawn()
                    .map_err(|e| format!("Failed to open URL: {}", e))?;
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open")
                    .arg(&url)
                    .spawn()
                    .map_err(|e| format!("Failed to open URL: {}", e))?;
            }
            Ok(())
        }
        "open_app" => {
            let app_name = cmd.action_payload.trim();
            info!("Executing voice command: open_app({})", app_name);
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd")
                    .args(["/c", "start", "", app_name])
                    .spawn()
                    .map_err(|e| format!("Failed to open app: {}", e))?;
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open")
                    .arg("-a")
                    .arg(app_name)
                    .spawn()
                    .map_err(|e| format!("Failed to open app: {}", e))?;
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open")
                    .arg(app_name)
                    .spawn()
                    .map_err(|e| format!("Failed to open app: {}", e))?;
            }
            Ok(())
        }
        "type_text" => {
            let text = cmd.action_payload.clone();
            info!("Executing voice command: type_text({})", text);
            let _ = app.run_on_main_thread({
                let app = app.clone();
                move || {
                    if let Err(e) = crate::utils::paste(text, app) {
                        error!("Failed to type text for command: {}", e);
                    }
                }
            }).map_err(|e| format!("Failed to run on main thread: {}", e))?;
            Ok(())
        }
        "run_script" => {
            let script = cmd.action_payload.trim();
            if script.is_empty() {
                return Err("Empty script payload".to_string());
            }
            info!("Executing voice command: run_script({})", script);
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd")
                    .args(["/c", script])
                    .spawn()
                    .map_err(|e| format!("Failed to run script: {}", e))?;
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("sh")
                    .args(["-c", script])
                    .spawn()
                    .map_err(|e| format!("Failed to run script: {}", e))?;
            }
            Ok(())
        }
        _ => Err(format!("Unknown action_type: {}", cmd.action_type)),
    }
}
