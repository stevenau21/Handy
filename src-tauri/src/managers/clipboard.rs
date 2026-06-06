use anyhow::{anyhow, Result};
use chrono::Utc;
use log::{debug, info};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_specta::Event;
use tokio::sync::Mutex;

/// Interval for polling the system clipboard for new text
const CLIPBOARD_POLL_INTERVAL_MS: u64 = 500;

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ClipboardSource {
    Ocr, Voice, Manual, Clipboard,
}

impl std::fmt::Display for ClipboardSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardSource::Ocr => write!(f, "ocr"),
            ClipboardSource::Voice => write!(f, "voice"),
            ClipboardSource::Manual => write!(f, "manual"),
            ClipboardSource::Clipboard => write!(f, "clipboard"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct PaginatedClipboard { pub entries: Vec<ClipboardEntry>, pub has_more: bool }

#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(tag = "action")]
pub enum ClipboardUpdatePayload {
    #[serde(rename = "added")] Added { entry: ClipboardEntry },
    #[serde(rename = "updated")] Updated { entry: ClipboardEntry },
    #[serde(rename = "deleted")] Deleted { id: i64 },
    #[serde(rename = "toggled")] Toggled { id: i64 },
    #[serde(rename = "cleared")] Cleared,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct ClipboardEntry {
    pub id: i64, pub text: String, pub note: Option<String>,
    pub timestamp: i64, pub saved: bool, pub source: String,
}

/// Event emitted when clipboard text is intercepted and waiting for user confirmation.
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct InterceptEvent {
    pub intercept_id: String,
    pub text: String,
    /// Source of the intercept: "ocr" for screenshot OCR, "clipboard" for text copy/voice paste
    pub source: String,
}

pub struct ClipboardManager {
    app_handle: AppHandle, db_path: PathBuf,
    last_clipboard_text: Mutex<Option<String>>,
    last_ocr_text: Mutex<Option<String>>,
    last_ocr_timestamp: Mutex<i64>,
    auto_track_running: Mutex<bool>,
    /// Pending intercepts: intercept_id → text, waiting for user to confirm/discard
    pending_intercepts: Mutex<HashMap<String, String>>,
}

impl ClipboardManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
        let db_path = app_data_dir.join("clipboard.db");
    let manager = Self { app_handle: app_handle.clone(), db_path, last_clipboard_text: Mutex::new(None), last_ocr_text: Mutex::new(None), last_ocr_timestamp: Mutex::new(0), auto_track_running: Mutex::new(false), pending_intercepts: Mutex::new(HashMap::new()) };
        manager.init_database()?;
        Ok(manager)
    }

    fn init_database(&self) -> Result<()> {
        info!("Initializing clipboard database at {:?}", self.db_path);
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clipboard_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL, note TEXT,
                timestamp INTEGER NOT NULL, saved BOOLEAN NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'manual');
             CREATE INDEX IF NOT EXISTS idx_clipboard_timestamp ON clipboard_entries(timestamp DESC);
             CREATE INDEX IF NOT EXISTS idx_clipboard_saved ON clipboard_entries(saved);")?;
        let has_note_column = conn.prepare("SELECT note FROM clipboard_entries LIMIT 0").is_ok();
        if !has_note_column { conn.execute_batch("ALTER TABLE clipboard_entries ADD COLUMN note TEXT;").ok(); }
        Ok(())
    }

    fn get_connection(&self) -> Result<Connection> {
        Connection::open(&self.db_path).map_err(|e| anyhow!("Failed to open clipboard DB: {}", e))
    }

    fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipboardEntry> {
        Ok(ClipboardEntry { id: row.get("id")?, text: row.get("text")?, note: row.get("note")?, timestamp: row.get("timestamp")?, saved: row.get("saved")?, source: row.get("source")? })
    }

    /// Normalize clipboard text to avoid false positive duplicates
    /// from CRLF/LF differences, trailing spaces, etc.
    fn normalize_text(text: &str) -> String {
        text.replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim_end()
            .to_string()
    }

    pub fn add_entry(&self, text: &str, source: ClipboardSource) -> Result<ClipboardEntry> {
        let normalized = Self::normalize_text(text);
        if normalized.is_empty() { return Err(anyhow!("Cannot add empty text to clipboard")); }
        let conn = self.get_connection()?;
        let source_str = source.to_string();
        conn.execute("INSERT INTO clipboard_entries (text, timestamp, saved, source) VALUES (?1, ?2, 0, ?3)", params![&normalized, Utc::now().timestamp(), source_str])?;
        let id = conn.last_insert_rowid();
        conn.execute("DELETE FROM clipboard_entries WHERE id NOT IN (SELECT id FROM clipboard_entries ORDER BY id DESC LIMIT 500)", [])?;
        let entry = ClipboardEntry { id, text: normalized, note: None, timestamp: Utc::now().timestamp(), saved: false, source: source_str };
        let _ = (ClipboardUpdatePayload::Added { entry: entry.clone() }).emit(&self.app_handle);
        Ok(entry)
    }

    pub async fn add_from_clipboard(&self, raw_text: &str) -> Result<ClipboardEntry> {
        let normalized = Self::normalize_text(raw_text);
        let mut last = self.last_clipboard_text.lock().await;
        if let Some(ref lt) = *last { if lt == &normalized { return Err(anyhow!("Same clipboard text as before")); } }
        *last = Some(normalized.clone()); drop(last);
        self.add_entry(&normalized, ClipboardSource::Clipboard)
    }

    pub fn get_entries(&self, cursor: Option<i64>, limit: Option<usize>, filter_saved: Option<bool>, search: Option<&str>) -> Result<PaginatedClipboard> {
        let conn = self.get_connection()?;
        let limit = limit.unwrap_or(30);
        let fetch_count = limit + 1;
        const COLS: &str = "SELECT id, text, note, timestamp, saved, source FROM clipboard_entries";
        macro_rules! q { ($stmt:expr, $params:expr) => { { let r = $stmt.query_map($params, Self::map_entry)?; r.collect::<std::result::Result<Vec<_>, _>>()? } }; }
        let entries = if let Some(qry) = search {
            let like = format!("%{}%", qry);
            match cursor {
                Some(c) => { let mut s = conn.prepare(&format!("{COLS} WHERE id < ?1 AND text LIKE ?2 ORDER BY id DESC LIMIT ?3"))?; q!(s, params![c, like, fetch_count]) }
                None => { let mut s = conn.prepare(&format!("{COLS} WHERE text LIKE ?1 ORDER BY id DESC LIMIT ?2"))?; q!(s, params![like, fetch_count]) }
            }
        } else if filter_saved == Some(true) {
            match cursor {
                Some(c) => { let mut s = conn.prepare(&format!("{COLS} WHERE id < ?1 AND saved = 1 ORDER BY id DESC LIMIT ?2"))?; q!(s, params![c, fetch_count]) }
                None => { let mut s = conn.prepare(&format!("{COLS} WHERE saved = 1 ORDER BY id DESC LIMIT ?1"))?; q!(s, params![fetch_count]) }
            }
        } else {
            match cursor {
                Some(c) => { let mut s = conn.prepare(&format!("{COLS} WHERE id < ?1 ORDER BY id DESC LIMIT ?2"))?; q!(s, params![c, fetch_count]) }
                None => { let mut s = conn.prepare(&format!("{COLS} ORDER BY id DESC LIMIT ?1"))?; q!(s, params![fetch_count]) }
            }
        };
        let has_more = entries.len() > limit;
        let mut entries = entries; if has_more { entries.pop(); }
        Ok(PaginatedClipboard { entries, has_more })
    }

    pub fn edit_entry(&self, id: i64, new_text: &str) -> Result<ClipboardEntry> {
        if new_text.trim().is_empty() { return Err(anyhow!("Cannot set empty text")); }
        let conn = self.get_connection()?;
        conn.execute("UPDATE clipboard_entries SET text = ?1 WHERE id = ?2", params![new_text, id])?;
        let entry: ClipboardEntry = conn.query_row("SELECT id, text, note, timestamp, saved, source FROM clipboard_entries WHERE id = ?1", params![id], Self::map_entry)?;
        let _ = (ClipboardUpdatePayload::Updated { entry: entry.clone() }).emit(&self.app_handle);
        Ok(entry)
    }

    pub fn set_note(&self, id: i64, note: Option<&str>) -> Result<ClipboardEntry> {
        let conn = self.get_connection()?;
        conn.execute("UPDATE clipboard_entries SET note = ?1 WHERE id = ?2", params![note, id])?;
        let entry: ClipboardEntry = conn.query_row("SELECT id, text, note, timestamp, saved, source FROM clipboard_entries WHERE id = ?1", params![id], Self::map_entry)?;
        let _ = (ClipboardUpdatePayload::Updated { entry: entry.clone() }).emit(&self.app_handle);
        Ok(entry)
    }

    pub fn toggle_saved(&self, id: i64) -> Result<()> {
        let conn = self.get_connection()?;
        let current: bool = conn.query_row("SELECT saved FROM clipboard_entries WHERE id = ?1", params![id], |row| row.get(0))?;
        conn.execute("UPDATE clipboard_entries SET saved = ?1 WHERE id = ?2", params![!current, id])?;
        let _ = (ClipboardUpdatePayload::Toggled { id }).emit(&self.app_handle);
        Ok(())
    }

    pub fn delete_entry(&self, id: i64) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute("DELETE FROM clipboard_entries WHERE id = ?1", params![id])?;
        let _ = (ClipboardUpdatePayload::Deleted { id }).emit(&self.app_handle);
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        let conn = self.get_connection()?;
        conn.execute("DELETE FROM clipboard_entries", [])?;
        let _ = (ClipboardUpdatePayload::Cleared).emit(&self.app_handle);
        Ok(())
    }

    /// Fallback clipboard reader using PowerShell — works when Tauri plugin can't lock clipboard.
    #[cfg(windows)]
    fn read_clipboard_text_powershell(&self) -> Result<String> {
        use std::os::windows::process::CommandExt;

        let mut command = std::process::Command::new("powershell");
        let output = command
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .args([
                "-Sta",
                "-NoProfile",
                "-ExecutionPolicy", "Bypass",
                "-Command",
                "Add-Type -AssemblyName System.Windows.Forms; [Console]::WriteLine([System.Windows.Forms.Clipboard]::GetText())"
            ])
            .output()
            .map_err(|e| anyhow!("PowerShell clipboard read failed: {}", e))?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Err(anyhow!("Clipboard empty or no text"))
        } else {
            Ok(text)
        }
    }

    /// Start auto-tracking the system clipboard. Tracks text and screenshots.
    pub async fn start_auto_track(self: Arc<Self>) {
        let mut running = self.auto_track_running.lock().await;
        if *running { return; }
        *running = true; drop(running);
        let app_handle = self.app_handle.clone();
        let manager = self;
        info!("Clipboard auto-tracker started");
        tokio::spawn(async move {
            // Throttle OCR checks to every 4 seconds (every 8 ticks at 500ms)
            let mut tick: u64 = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(CLIPBOARD_POLL_INTERVAL_MS)).await;
                if !*manager.auto_track_running.lock().await {
                    info!("Clipboard auto-tracker stopped");
                    break;
                }
                tick += 1;

                // --- TEXT TRACKING ---
                let text: String = match app_handle.clipboard().read_text() {
                    Ok(t) if !t.is_empty() => {
                        debug!("Tauri plugin read {} chars from clipboard", t.len());
                        t
                    }
                    Ok(_) => {
                        // No text — try OCR on image below
                        String::new()
                    }
                    Err(e) => {
                        debug!("Tauri plugin clipboard read failed ({}), trying PowerShell fallback", e);
                        let fallback: Result<String>;
                        #[cfg(windows)]
                        { fallback = manager.read_clipboard_text_powershell(); }
                        #[cfg(not(windows))]
                        { fallback = Err(anyhow!("non-Windows, no fallback")); }
                        match fallback {
                            Ok(t) if !t.is_empty() => {
                                info!("PowerShell fallback read {} chars from clipboard", t.len());
                                t
                            }
                            Ok(_) => String::new(),
                            Err(e2) => {
                                debug!("PowerShell fallback also failed: {}", e2);
                                String::new()
                            }
                        }
                    }
                };
                if !text.trim().is_empty() && text.len() < 100_000 {
                    match manager.intercept_clipboard_text(&text).await {
                        Ok(Some(id)) => info!("Clipboard intercepted, waiting for user: {}", id),
                        Ok(None) => debug!("Clipboard text dedup or empty"),
                        Err(e) => debug!("Clipboard intercept error: {}", e),
                    }
                }

                // --- IMAGE / OCR TRACKING ---
                // Every 4 seconds, try OCR on any image in clipboard (screenshots from Win+Shift+S)
                if tick % 8 == 0 {
                    debug!("Checking clipboard for image to OCR...");
                    match crate::ocr::ocr_clipboard_image().await {
                        Ok(text) if !text.trim().is_empty() => {
                            let normalized = Self::normalize_text(&text);

                            // Debounce: skip if same OCR text within last 10 seconds
                            let now = Utc::now().timestamp();
                            let should_skip = {
                                let mut last_ocr_text_guard = manager.last_ocr_text.lock().await;
                                let mut last_ocr_ts_guard = manager.last_ocr_timestamp.lock().await;
                                let skip = last_ocr_text_guard.as_ref().map_or(false, |t| t == &normalized)
                                    && (now - *last_ocr_ts_guard) < 10;
                                if !skip {
                                    *last_ocr_text_guard = Some(normalized.clone());
                                    *last_ocr_ts_guard = now;
                                }
                                skip
                            };
                            if should_skip {
                                debug!("OCR duplicate text within 10s, skipping");
                                continue;
                            }

                            if let Some(id) = manager.intercept_ocr_text(&normalized).await {
                                info!("OCR intercepted, waiting for user: {}", id);
                            }
                        }
                        Ok(_) => {
                            debug!("No text found in clipboard image");
                        }
                        Err(e) => {
                            debug!("Auto-OCR skipped: {}", e);
                        }
                    }
                }
            }
        });
    }

    /// Core intercept creation: store pending + emit event. No dedup logic here.
    async fn create_intercept(&self, text: &str, source: &str) -> String {
        let normalized = Self::normalize_text(text);
        let intercept_id = format!("icpt_{}", Utc::now().timestamp_millis());
        self.pending_intercepts.lock().await.insert(intercept_id.clone(), normalized.clone());
        let _ = InterceptEvent { intercept_id: intercept_id.clone(), text: normalized.clone(), source: source.to_string() }.emit(&self.app_handle);
        info!("Clipboard intercept {} emitted ({} chars, source={})", intercept_id, normalized.len(), source);
        intercept_id
    }

    pub async fn intercept_clipboard_text(&self, raw_text: &str) -> Result<Option<String>> {
        let normalized = Self::normalize_text(raw_text);
        if normalized.is_empty() { return Ok(None); }

        // Dedup: skip if same text as last tracked
        let mut last = self.last_clipboard_text.lock().await;
        if let Some(ref lt) = *last { if lt == &normalized { return Ok(None); } }
        *last = Some(normalized.clone());
        drop(last);

        let intercept_id = self.create_intercept(&normalized, "clipboard").await;
        Ok(Some(intercept_id))
    }

    /// OCR intercept: bypass last_clipboard_text dedup (OCR has its own 10s debounce).
    pub async fn intercept_ocr_text(&self, text: &str) -> Option<String> {
        let normalized = Self::normalize_text(text);
        if normalized.is_empty() { return None; }
        let intercept_id = self.create_intercept(&normalized, "ocr").await;
        Some(intercept_id)
    }

    /// User confirmed: save the intercepted text to the DB.
    pub async fn confirm_intercept(&self, intercept_id: &str) -> Result<Option<ClipboardEntry>> {
        let text = self.pending_intercepts.lock().await.remove(intercept_id);
        match text {
            Some(t) => {
                let entry = self.add_entry(&t, ClipboardSource::Clipboard)?;
                info!("Intercept {} confirmed, saved as entry {}", intercept_id, entry.id);
                Ok(Some(entry))
            }
            None => {
                debug!("Intercept {} not found (expired or already handled)", intercept_id);
                Ok(None)
            }
        }
    }

    /// Confirm with edited text instead of the original intercepted text.
    pub async fn confirm_intercept_with_text(&self, intercept_id: &str, edited_text: &str) -> Result<Option<ClipboardEntry>> {
        // Remove the pending intercept so it can't be re-used
        self.pending_intercepts.lock().await.remove(intercept_id);
        let normalized = Self::normalize_text(edited_text);
        if normalized.is_empty() {
            return Ok(None);
        }
        let entry = self.add_entry(&normalized, ClipboardSource::Clipboard)?;
        info!("Intercept {} confirmed with edited text, saved as entry {}", intercept_id, entry.id);
        Ok(Some(entry))
    }

    /// User confirmed with a voice note: save with note attached.
    pub async fn confirm_intercept_with_note(&self, intercept_id: &str, note: &str) -> Result<Option<ClipboardEntry>> {
        let text = self.pending_intercepts.lock().await.remove(intercept_id);
        match text {
            Some(t) => {
                let normalized = Self::normalize_text(&t);
                if normalized.is_empty() { return Ok(None); }
                let conn = self.get_connection()?;
                conn.execute(
                    "INSERT INTO clipboard_entries (text, note, timestamp, saved, source) VALUES (?1, ?2, ?3, 0, 'clipboard')",
                    params![&normalized, note, Utc::now().timestamp()],
                )?;
                let id = conn.last_insert_rowid();
                conn.execute("DELETE FROM clipboard_entries WHERE id NOT IN (SELECT id FROM clipboard_entries ORDER BY id DESC LIMIT 500)", [])?;
                let entry = ClipboardEntry { id, text: normalized, note: Some(note.to_string()), timestamp: Utc::now().timestamp(), saved: false, source: "clipboard".to_string() };
                let _ = (ClipboardUpdatePayload::Added { entry: entry.clone() }).emit(&self.app_handle);
                info!("Intercept {} confirmed with voice note, saved as entry {}", intercept_id, entry.id);
                Ok(Some(entry))
            }
            None => {
                debug!("Intercept {} not found (expired or already handled)", intercept_id);
                Ok(None)
            }
        }
    }

    /// User discarded: remove pending intercept without saving.
    pub async fn discard_intercept(&self, intercept_id: &str) {
        let removed = self.pending_intercepts.lock().await.remove(intercept_id);
        if removed.is_some() {
            info!("Intercept {} discarded", intercept_id);
        }
    }

    pub async fn stop_auto_track(&self) {
        let mut running = self.auto_track_running.lock().await;
        *running = false;
    }
}