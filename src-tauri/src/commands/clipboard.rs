use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::managers::clipboard::{
    ClipboardEntry, ClipboardManager, ClipboardSource, PaginatedClipboard,
};

#[tauri::command]
#[specta::specta]
pub async fn get_clipboard_entries(
    _app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    cursor: Option<i64>,
    limit: Option<usize>,
    filter_saved: Option<bool>,
    search: Option<String>,
) -> Result<PaginatedClipboard, String> {
    clipboard_manager
        .get_entries(cursor, limit, filter_saved, search.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn add_clipboard_entry(
    _app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    text: String,
    source: String,
) -> Result<ClipboardEntry, String> {
    let source = match source.as_str() {
        "ocr" => ClipboardSource::Ocr,
        "voice" => ClipboardSource::Voice,
        "manual" => ClipboardSource::Manual,
        "clipboard" => ClipboardSource::Clipboard,
        _ => ClipboardSource::Manual,
    };

    clipboard_manager
        .add_entry(&text, source)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn edit_clipboard_entry(
    _app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    id: i64,
    new_text: String,
) -> Result<ClipboardEntry, String> {
    clipboard_manager
        .edit_entry(id, &new_text)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn set_clipboard_entry_note(
    _app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    id: i64,
    note: Option<String>,
) -> Result<ClipboardEntry, String> {
    clipboard_manager
        .set_note(id, note.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn toggle_clipboard_entry_saved(
    _app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    id: i64,
) -> Result<(), String> {
    clipboard_manager
        .toggle_saved(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_clipboard_entry(
    _app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    id: i64,
) -> Result<(), String> {
    clipboard_manager
        .delete_entry(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn clear_all_clipboard_entries(
    _app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
) -> Result<(), String> {
    clipboard_manager
        .clear_all()
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn copy_to_clipboard(
    app: AppHandle,
    text: String,
) -> Result<(), String> {
    app.clipboard()
        .write_text(&text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))
}


#[tauri::command]
#[specta::specta]
pub async fn ocr_grab_screen(
    app: AppHandle,
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
) -> Result<ClipboardEntry, String> {
    let text = crate::ocr::ocr_clipboard_image().await
        .map_err(|e| format!("OCR failed: {}", e))?;

    // Put OCR result on clipboard
    app.clipboard()
        .write_text(&text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    clipboard_manager
        .add_entry(&text, ClipboardSource::Ocr)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn start_clipboard_auto_track(
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
) -> Result<(), String> {
    let manager = (*clipboard_manager).clone();
    Arc::clone(&manager).start_auto_track().await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn stop_clipboard_auto_track(
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
) -> Result<(), String> {
    clipboard_manager.stop_auto_track().await;
    Ok(())
}

/// Confirm a clipboard intercept — saves the text to the clipboard DB.
#[tauri::command]
#[specta::specta]
pub async fn confirm_clipboard_intercept(
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    intercept_id: String,
) -> Result<Option<ClipboardEntry>, String> {
    clipboard_manager
        .confirm_intercept(&intercept_id)
        .await
        .map_err(|e| e.to_string())
}

/// Confirm a clipboard intercept with a voice note attached.
#[tauri::command]
#[specta::specta]
pub async fn confirm_clipboard_intercept_with_note(
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    intercept_id: String,
    note: String,
) -> Result<Option<ClipboardEntry>, String> {
    clipboard_manager
        .confirm_intercept_with_note(&intercept_id, &note)
        .await
        .map_err(|e| e.to_string())
}

/// Confirm a clipboard intercept with edited text (user modified in the popup).
#[tauri::command]
#[specta::specta]
pub async fn confirm_clipboard_intercept_with_text(
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    intercept_id: String,
    edited_text: String,
) -> Result<Option<ClipboardEntry>, String> {
    clipboard_manager
        .confirm_intercept_with_text(&intercept_id, &edited_text)
        .await
        .map_err(|e| e.to_string())
}

/// Discard a clipboard intercept without saving.
#[tauri::command]
#[specta::specta]
pub async fn discard_clipboard_intercept(
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
    intercept_id: String,
) -> Result<(), String> {
    clipboard_manager
        .discard_intercept(&intercept_id)
        .await;
    Ok(())
}

/// FIX 5: Force a manual clipboard refresh — clears dedup state and re-reads clipboard.
/// Returns a status message for the UI to display.
#[tauri::command]
#[specta::specta]
pub async fn force_refresh_clipboard(
    clipboard_manager: State<'_, Arc<ClipboardManager>>,
) -> Result<String, String> {
    clipboard_manager
        .force_refresh_clipboard()
        .await
        .map_err(|e| e.to_string())
}
