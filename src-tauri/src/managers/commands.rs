use crate::settings::VoiceCommand;
use log::{debug, error, info, warn};
use tauri::AppHandle;

/// Check if transcribed text is a voice command.
pub fn parse_command(text: &str, commands: &[VoiceCommand], wake_phrase: &str) -> Option<VoiceCommand> {
    let trimmed = text.trim();
    let after_wake = if trimmed.to_lowercase().starts_with(wake_phrase) {
        trimmed[wake_phrase.len()..].trim()
    } else {
        trimmed
    };
    if after_wake.is_empty() { return None; }
    for cmd in commands {
        if !cmd.enabled { continue; }
        if after_wake.to_lowercase().contains(&cmd.phrase.to_lowercase()) {
            debug!("Voice command matched: '{}' -> '{}'", after_wake, cmd.phrase);
            return Some(cmd.clone());
        }
    }
    None
}

fn extract_query_after_phrase(text: &str, phrase: &str) -> String {
    let tl = text.to_lowercase();
    let pl = phrase.to_lowercase();
    if let Some(pos) = tl.find(&pl) {
        let after = tl[pos + pl.len()..].trim();
        for prefix in &["and ", "for ", "then ", "search for ", "search "] {
            if after.starts_with(prefix) {
                return after[prefix.len()..].trim().to_string();
            }
        }
        if !after.is_empty() { return after.to_string(); }
    }
    String::new()
}

pub fn execute_command(app: &AppHandle, cmd: &VoiceCommand, transcribed_text: Option<&str>) -> Result<(), String> {
    match cmd.action_type.as_str() {
        "open_url" => {
            let url = cmd.action_payload.trim();
            if url.is_empty() { return Err("Empty URL payload".to_string()); }
            let url = if !url.starts_with("http://") && !url.starts_with("https://") { format!("https://{}", url) } else { url.to_string() };
            info!("Executing voice command: open_url({})", url);
            #[cfg(target_os = "windows")]
            { let _ = std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn().map_err(|e| format!("Failed to open URL: {}", e))?; }
            #[cfg(target_os = "macos")]
            { let _ = std::process::Command::new("open").arg(&url).spawn().map_err(|e| format!("Failed to open URL: {}", e))?; }
            #[cfg(target_os = "linux")]
            { let _ = std::process::Command::new("xdg-open").arg(&url).spawn().map_err(|e| format!("Failed to open URL: {}", e))?; }
            Ok(())
        }
        "open_app" => {
            let app_name = cmd.action_payload.trim();
            if app_name.is_empty() { return Err("Empty app name payload".to_string()); }
            info!("Executing voice command: open_app({})", app_name);
            #[cfg(target_os = "windows")]
            { open_with_shell_execute(app_name); }
            #[cfg(target_os = "macos")]
            { let _ = std::process::Command::new("open").arg("-a").arg(app_name).spawn().map_err(|e| format!("Failed to open app: {}", e))?; }
            #[cfg(target_os = "linux")]
            { let _ = std::process::Command::new("xdg-open").arg(app_name).spawn().map_err(|e| format!("Failed to open app: {}", e))?; }
            Ok(())
        }
        "type_text" => {
            let text = cmd.action_payload.clone();
            info!("Executing voice command: type_text({})", text);
            let _ = app.run_on_main_thread({ let app = app.clone(); move || { if let Err(e) = crate::utils::paste(text, app) { error!("Failed to type text for command: {}", e); } } }).map_err(|e| format!("Failed to run on main thread: {}", e))?;
            Ok(())
        }
        "send_message" => {
            let username = cmd.action_payload.trim();
            if username.is_empty() { return Err("Empty username payload".to_string()); }
            let message = transcribed_text
                .and_then(|t| {
                    let q = extract_query_after_phrase(t, &cmd.phrase);
                    if q.is_empty() { None } else { Some(q) }
                })
                .unwrap_or_default();

            if message.is_empty() {
                return Err("No message text extracted from transcription".to_string());
            }

            info!("Executing voice command: send_message(to='{}', msg='{}')", username, message);
            #[cfg(target_os = "windows")]
            {
                let cmd_str = format!("dark-send -c \"{}\" \"{}\"", username, message);
                let _ = std::process::Command::new("cmd")
                    .args(["/c", &cmd_str])
                    .spawn()
                    .map_err(|e| format!("Failed to send message via dark-send: {}", e))?;
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("dark-send")
                    .args(["-c", username, &message])
                    .spawn()
                    .map_err(|e| format!("Failed to send message via dark-send: {}", e))?;
            }
            Ok(())
        }
        "search_url" => {
            let base_url = cmd.action_payload.trim();
            if base_url.is_empty() { return Err("Empty search URL payload".to_string()); }
            let query = transcribed_text.and_then(|t| { let q = extract_query_after_phrase(t, &cmd.phrase); if q.is_empty() { None } else { Some(q) } }).unwrap_or_default();
            let url = if !query.is_empty() {
                let encoded: String = query.split_whitespace().collect::<Vec<_>>().join("+");
                format!("{}{}", base_url.trim_end_matches('?'), encoded)
            } else {
                if !base_url.starts_with("http://") && !base_url.starts_with("https://") { format!("https://{}", base_url) } else { base_url.to_string() }
            };
            info!("Executing voice command: search_url(query='{}', url={})", query, url);
            #[cfg(target_os = "windows")]
            { let _ = std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn().map_err(|e| format!("Failed to open search URL: {}", e))?; }
            #[cfg(target_os = "macos")]
            { let _ = std::process::Command::new("open").arg(&url).spawn().map_err(|e| format!("Failed to open search URL: {}", e))?; }
            #[cfg(target_os = "linux")]
            { let _ = std::process::Command::new("xdg-open").arg(&url).spawn().map_err(|e| format!("Failed to open search URL: {}", e))?; }
            Ok(())
        }
        "run_script" => {
            let mut script = cmd.action_payload.trim().to_string();
            if script.is_empty() { return Err("Empty script payload".to_string()); }
            let lowered = script.to_lowercase();
            if lowered.starts_with("cmd.exe /c ") { script = script[11..].to_string(); } else if lowered.starts_with("cmd /c ") { script = script[7..].to_string(); }
            info!("Executing voice command: run_script({})", script);
            #[cfg(target_os = "windows")]
            {
                let mut file_or_cmd = script.clone();
                let s_lower = file_or_cmd.to_lowercase();
                if s_lower.starts_with("start \"\" ") { file_or_cmd = file_or_cmd[9..].to_string(); }
                else if s_lower.starts_with("start ") { let after = &file_or_cmd[6..]; if let Some(pos) = after.find(' ') { file_or_cmd = after[pos + 1..].to_string(); } }
                open_with_shell_execute(&file_or_cmd);
            }
            #[cfg(not(target_os = "windows"))]
            { let _ = std::process::Command::new("sh").args(["-c", &script]).spawn().map_err(|e| format!("Failed to run script: {}", e))?; }
            Ok(())
        }
        _ => Err(format!("Unknown action_type: {}", cmd.action_type)),
    }
}

#[cfg(target_os = "windows")]
fn open_with_shell_execute(path: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let hinstance = unsafe { ShellExecuteW(None, PCWSTR::from_raw("open\0".encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>().as_ptr()), PCWSTR::from_raw(wide.as_ptr()), None, None, SW_SHOWNORMAL) };
    let code = (hinstance.0 as *const core::ffi::c_void) as isize;
    if code <= 32 { warn!("ShellExecuteW failed (code {}) for path: {}", code, path); } else { info!("ShellExecuteW succeeded for: {}", path); }
}
