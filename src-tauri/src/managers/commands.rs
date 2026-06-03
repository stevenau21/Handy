use crate::settings::VoiceCommand;
use log::{debug, error, info, warn};
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
            if app_name.is_empty() {
                return Err("Empty app name payload".to_string());
            }
            info!("Executing voice command: open_app({})", app_name);
            #[cfg(target_os = "windows")]
            {
                // Use ShellExecuteW to open apps by name or path.
                // This handles paths with spaces and .lnk files correctly.
                open_with_shell_execute(app_name);
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
            let mut script = cmd.action_payload.trim().to_string();
            if script.is_empty() {
                return Err("Empty script payload".to_string());
            }
            // Strip accidental "cmd /c " or "cmd.exe /c " prefix since we
            // already invoke through cmd on Windows.
            let lowered = script.to_lowercase();
            if lowered.starts_with("cmd.exe /c ") {
                script = script[11..].to_string();
            } else if lowered.starts_with("cmd /c ") {
                script = script[7..].to_string();
            }
            info!("Executing voice command: run_script({})", script);

            #[cfg(target_os = "windows")]
            {
                // Strip "start "" prefix — ShellExecuteW opens files directly
                // so `start` is unnecessary and causes quoting issues.
                let mut file_or_cmd = script.clone();
                let s_lower = file_or_cmd.to_lowercase();
                if s_lower.starts_with("start \"\" ") {
                    file_or_cmd = file_or_cmd[9..].to_string();
                } else if s_lower.starts_with("start ") {
                    // start <window_title> <path> — skip window title
                    let after_start = &file_or_cmd[6..];
                    if let Some(pos) = after_start.find(' ') {
                        file_or_cmd = after_start[pos + 1..].to_string();
                    }
                }
                open_with_shell_execute(&file_or_cmd);
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("sh")
                    .args(["-c", &script])
                    .spawn()
                    .map_err(|e| format!("Failed to run script: {}", e))?;
            }
            Ok(())
        }
        _ => Err(format!("Unknown action_type: {}", cmd.action_type)),
    }
}

/// On Windows, use ShellExecuteW to open a file, .lnk shortcut, or app.
/// This avoids the quoting issues that plague std::process::Command with cmd.exe.
#[cfg(target_os = "windows")]
fn open_with_shell_execute(path: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // Convert path to null-terminated UTF-16
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    let hinstance = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::from_raw("open\0".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>().as_ptr()),
            PCWSTR::from_raw(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };

    // ShellExecuteW returns an HINSTANCE (raw pointer wrapper).
    // Values <= 32 indicate error (HINSTANCE_ERROR).
    let code = (hinstance.0 as *const core::ffi::c_void) as isize;
    if code <= 32 {
        warn!("ShellExecuteW failed (code {}) for path: {}", code, path);
    } else {
        info!("ShellExecuteW succeeded for: {}", path);
    }
}