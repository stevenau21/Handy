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

/// How often (in ticks) to run OCR on clipboard images.
/// 4 ticks × 500ms = 2 seconds (was 8 ticks = 4s).
const OCR_CHECK_INTERVAL_TICKS: u64 = 4;

/// How often (in ticks) to force a PowerShell cross-check to detect stale Tauri reads.
/// 60 ticks × 500ms = 30 seconds.
const FORCE_REFRESH_INTERVAL_TICKS: u64 = 60;

/// Number of times the Tauri plugin must return the same text as our dedup state
/// before we force a PowerShell refresh to break a potential stale-read loop.
/// Reduced to 1 for immediate cross-check — when Tauri returns cached text,
/// we verify with PowerShell right away instead of waiting 1.5s (3×500ms).
const STALE_READ_THRESHOLD: u32 = 1;

/// DEDUP TIMEOUT: How many seconds before we clear dedup state and treat
/// the next clipboard content as new. 
/// 
/// NOTE: This must be longer than typical user reaction time to the intercept
/// modal, or the same clipboard text will be re-intercepted while the user is
/// still reading the dialog, causing an infinite "pops back up" loop.
/// 120s gives users ample time to read and respond.
const DEDUP_TIMEOUT_SECONDS: i64 = 120;

/// HEALTH CHECK: If no successful clipboard read for this many seconds,
/// log a warning and force-reset state (monitor may be stalled).
const HEALTH_CHECK_STALL_SECONDS: i64 = 15;

/// MAX RETRIES when Tauri returns empty before falling back to PowerShell.
const TAURI_EMPTY_RETRIES: u32 = 2;

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
    last_clipboard_timestamp: Mutex<i64>,
    last_ocr_text: Mutex<Option<String>>,
    last_ocr_timestamp: Mutex<i64>,
    auto_track_running: Mutex<bool>,
    /// Consecutive clipboard-read failures to detect stuck API state
    consecutive_read_failures: Mutex<u32>,
    /// Consecutive times the Tauri plugin returned the same text as our dedup state.
    /// When this hits STALE_READ_THRESHOLD we force a PowerShell refresh.
    stale_read_count: Mutex<u32>,
    /// Pending intercepts: intercept_id → text, waiting for user to confirm/discard
    pending_intercepts: Mutex<HashMap<String, String>>,
    /// When true, the clipboard monitor skips processing.
    /// Set during paste operations to prevent the clipboard-restore step
    /// from triggering a spurious intercept event.
    suppress_monitoring: std::sync::Mutex<bool>,
    /// Consecutive ticks where Tauri returned empty text.
    /// After TAURI_EMPTY_THRESHOLD ticks, we force a PowerShell cross-check
    /// to detect clipboard content that Tauri is missing.
    tauri_empty_count: Mutex<u32>,
    /// Timestamp of last successful clipboard read (for health monitoring)
    last_successful_read_ts: Mutex<i64>,
}

impl ClipboardManager {
    pub fn new(app_handle: &AppHandle) -> Result<Self> {
        let app_data_dir = crate::portable::app_data_dir(app_handle)?;
        let db_path = app_data_dir.join("clipboard.db");

        // Pre-seed with current clipboard content to avoid treating old
        // clipboard contents as a "new" intercept on app startup.
        let initial_clipboard: Option<String> = app_handle
            .clipboard()
            .read_text()
            .ok()
            .filter(|t| !t.trim().is_empty())
            .map(|t| Self::normalize_text(&t));

    let manager = Self {
        app_handle: app_handle.clone(),
        db_path,
        last_clipboard_text: Mutex::new(initial_clipboard.clone()),
        last_clipboard_timestamp: Mutex::new(if initial_clipboard.is_some() { Utc::now().timestamp() } else { 0 }),
        last_ocr_text: Mutex::new(None),
        last_ocr_timestamp: Mutex::new(0),
        auto_track_running: Mutex::new(false),
        consecutive_read_failures: Mutex::new(0),
        stale_read_count: Mutex::new(0),
        pending_intercepts: Mutex::new(HashMap::new()),
        suppress_monitoring: std::sync::Mutex::new(false),
        tauri_empty_count: Mutex::new(0),
        last_successful_read_ts: Mutex::new(Utc::now().timestamp()),
    };
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
        info!("Clipboard auto-tracker started (OCR every {}ms, force-refresh every {}ms)",
            OCR_CHECK_INTERVAL_TICKS * CLIPBOARD_POLL_INTERVAL_MS,
            FORCE_REFRESH_INTERVAL_TICKS * CLIPBOARD_POLL_INTERVAL_MS);
        tokio::spawn(async move {
            let mut tick: u64 = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(CLIPBOARD_POLL_INTERVAL_MS)).await;
                if !*manager.auto_track_running.lock().await {
                    info!("Clipboard auto-tracker stopped");
                    break;
                }
                tick += 1;

                let now = Utc::now().timestamp();

                // --- TEXT TRACKING ---
                // Try Tauri plugin first, but ALWAYS fall back to PowerShell on Windows
                // when Tauri returns empty — the Tauri plugin frequently returns empty
                // when the clipboard is locked by another process (e.g., screenshot tools).
                let mut read_failed = false;
                let mut tauri_text: Option<String> = None;
                let text: String = match app_handle.clipboard().read_text() {
                    Ok(t) if !t.is_empty() => {
                        info!("[CLIPBOARD] Tauri plugin read {} chars", t.len());
                        tauri_text = Some(t.clone());
                        // Reset empty counter since Tauri is working
                        *manager.tauri_empty_count.lock().await = 0;
                        t
                    }
                    Ok(_) => {
                        // Tauri returned empty — track consecutive empties
                        let mut empty_count = manager.tauri_empty_count.lock().await;
                        *empty_count += 1;
                        let ec = *empty_count;
                        drop(empty_count);
                        info!("[CLIPBOARD] Tauri returned empty (consecutive #{}), trying PowerShell fallback", ec);
                        // Always try PowerShell when Tauri returns empty on Windows
                        #[cfg(windows)]
                        {
                            match manager.read_clipboard_text_powershell() {
                                Ok(ps_text) if !ps_text.trim().is_empty() => {
                                    info!("[CLIPBOARD] PowerShell found {} chars that Tauri missed!", ps_text.len());
                                    // Reset empty counter since we found content
                                    *manager.tauri_empty_count.lock().await = 0;
                                    ps_text
                                }
                                Ok(_) => {
                                    info!("[CLIPBOARD] PowerShell also returned empty — clipboard truly empty");
                                    String::new()
                                }
                                Err(e) => {
                                    info!("[CLIPBOARD] PowerShell fallback failed: {}", e);
                                    String::new()
                                }
                            }
                        }
                        #[cfg(not(windows))]
                        { String::new() }
                    }
                    Err(e) => {
                        read_failed = true;
                        info!("[CLIPBOARD] Tauri read failed ({}), trying PowerShell fallback", e);
                        let fallback: Result<String>;
                        #[cfg(windows)]
                        { fallback = manager.read_clipboard_text_powershell(); }
                        #[cfg(not(windows))]
                        { fallback = Err(anyhow!("non-Windows, no fallback")); }
                        match fallback {
                            Ok(t) if !t.is_empty() => {
                                info!("[CLIPBOARD] PowerShell fallback read {} chars", t.len());
                                *manager.tauri_empty_count.lock().await = 0;
                                t
                            }
                            Ok(_) => {
                                info!("[CLIPBOARD] PowerShell fallback returned empty");
                                String::new()
                            }
                            Err(e2) => {
                                info!("[CLIPBOARD] PowerShell fallback also failed: {}", e2);
                                String::new()
                            }
                        }
                    }
                };

                // --- STALE-READ DETECTION ---
                // If the Tauri plugin returned text that matches our dedup state,
                // it might be a stale/cached read. Track consecutive matches.
                {
                    let last_text_guard = manager.last_clipboard_text.lock().await;
                    let mut stale_count = manager.stale_read_count.lock().await;
                    if let (Some(ref last), Some(ref tauri)) = (&*last_text_guard, &tauri_text) {
                        let normalized_tauri = Self::normalize_text(tauri);
                        if normalized_tauri == *last {
                            *stale_count += 1;
                            info!("[CLIPBOARD] Stale read #{} — Tauri returned same text as dedup state ({} chars)",
                                *stale_count, last.len());
                        } else {
                            // Different text — reset stale counter
                            if *stale_count > 0 {
                                info!("[CLIPBOARD] Stale counter reset (was {}) — Tauri returned different text", *stale_count);
                            }
                            *stale_count = 0;
                        }
                    } else {
                        *stale_count = 0;
                    }
                    drop(stale_count);
                    drop(last_text_guard);
                }

                // --- PERIODIC FORCE-REFRESH ---
                // Every 30s, cross-check with PowerShell to detect stale Tauri reads.
                // If PowerShell returns different text than our dedup state, use it.
                if tick % FORCE_REFRESH_INTERVAL_TICKS == 0 {
                    info!("[CLIPBOARD] Periodic force-refresh: cross-checking with PowerShell");
                    #[cfg(windows)]
                    {
                        match manager.read_clipboard_text_powershell() {
                            Ok(ps_text) if !ps_text.trim().is_empty() => {
                                let normalized_ps = Self::normalize_text(&ps_text);
                                let last_text_guard = manager.last_clipboard_text.lock().await;
                                let is_new = last_text_guard.as_ref().map_or(true, |lt| lt != &normalized_ps);
                                drop(last_text_guard);
                                if is_new {
                                    info!("[CLIPBOARD] Force-refresh found NEW text via PowerShell ({} chars) — Tauri may be stale",
                                        normalized_ps.len());
                                    // Use the PowerShell text instead of whatever Tauri returned
                                    if normalized_ps.len() < 100_000 {
                                        match manager.intercept_clipboard_text(&normalized_ps).await {
                                            Ok(Some(id)) => info!("[CLIPBOARD] Force-refresh intercepted: {}", id),
                                            Ok(None) => info!("[CLIPBOARD] Force-refresh text deduped"),
                                            Err(e) => info!("[CLIPBOARD] Force-refresh intercept error: {}", e),
                                        }
                                    }
                                } else {
                                    info!("[CLIPBOARD] Force-refresh: PowerShell text matches dedup state ({} chars) — OK",
                                        normalized_ps.len());
                                }
                            }
                            Ok(_) => {
                                info!("[CLIPBOARD] Force-refresh: PowerShell returned empty clipboard");
                            }
                            Err(e) => {
                                info!("[CLIPBOARD] Force-refresh: PowerShell failed: {}", e);
                            }
                        }
                    }
                }

                // --- STALE-READ RECOVERY ---
                // If Tauri returned the same text as dedup state STALE_READ_THRESHOLD times,
                // force a PowerShell read to break the loop.
                let mut stale_recovery_intercepted = false;
                let force_ps_read = {
                    let stale_count = manager.stale_read_count.lock().await;
                    *stale_count >= STALE_READ_THRESHOLD
                };
                if force_ps_read && !text.trim().is_empty() {
                    info!("[CLIPBOARD] {} stale reads — forcing PowerShell refresh to break potential loop",
                        STALE_READ_THRESHOLD);
                    #[cfg(windows)]
                    {
                        match manager.read_clipboard_text_powershell() {
                            Ok(ps_text) if !ps_text.trim().is_empty() => {
                                let normalized_ps = Self::normalize_text(&ps_text);
                                let last_text_guard = manager.last_clipboard_text.lock().await;
                                let is_new = last_text_guard.as_ref().map_or(true, |lt| lt != &normalized_ps);
                                drop(last_text_guard);
                                if is_new {
                                    info!("[CLIPBOARD] Stale-recovery: PowerShell found NEW text ({} chars)",
                                        normalized_ps.len());
                                    if normalized_ps.len() < 100_000 {
                                        match manager.intercept_clipboard_text(&normalized_ps).await {
                                            Ok(Some(id)) => {
                                                info!("[CLIPBOARD] Stale-recovery intercepted: {}", id);
                                                stale_recovery_intercepted = true;
                                            }
                                            Ok(None) => info!("[CLIPBOARD] Stale-recovery text deduped"),
                                            Err(e) => info!("[CLIPBOARD] Stale-recovery intercept error: {}", e),
                                        }
                                    }
                                } else {
                                    info!("[CLIPBOARD] Stale-recovery: PowerShell text matches dedup — clipboard truly unchanged");
                                }
                            }
                            Ok(_) => {
                                info!("[CLIPBOARD] Stale-recovery: PowerShell returned empty — clipboard cleared externally");
                                // Clear dedup state so next copy is caught
                                *manager.last_clipboard_text.lock().await = None;
                                *manager.last_clipboard_timestamp.lock().await = 0;
                            }
                            Err(e) => {
                                info!("[CLIPBOARD] Stale-recovery: PowerShell failed: {}", e);
                            }
                        }
                    }
                    // Reset stale counter after recovery attempt
                    *manager.stale_read_count.lock().await = 0;
                }

                let mut last_ts = manager.last_clipboard_timestamp.lock().await;
                let mut failures = manager.consecutive_read_failures.lock().await;
                let seconds_since_last_text = now.saturating_sub(*last_ts);

                // FIX 1: Reduced dedup timeout from 30s → 10s.
                // If we haven't seen NEW text for 10s, clear dedup state so
                // the next copy is always treated as new. This fixes the
                // "stuck dedup" bug where closing+reopening Handy fixes it.
                // Also catches rapid copies of similar content (screenshots, edited text).
                if seconds_since_last_text > DEDUP_TIMEOUT_SECONDS && manager.last_clipboard_text.lock().await.is_some() {
                    info!("[CLIPBOARD] Dedup timeout ({}s) — clearing dedup state (last text was {}s ago)", DEDUP_TIMEOUT_SECONDS, seconds_since_last_text);
                    *manager.last_clipboard_text.lock().await = None;
                    *last_ts = 0;
                    // Also reset stale counter on timeout
                    *manager.stale_read_count.lock().await = 0;
                }
                // FIX 3: Health check — track last time we successfully polled clipboard.
                // Update last_successful_read_ts whenever we get text (even if deduped).
                if !text.is_empty() || tauri_text.is_some() {
                    *manager.last_successful_read_ts.lock().await = now;
                }
                let last_successful = *manager.last_successful_read_ts.lock().await;
                let seconds_since_successful_poll = now.saturating_sub(last_successful);
                // Log every 15s if we haven't successfully read from clipboard
                if seconds_since_successful_poll > 0 && seconds_since_successful_poll % HEALTH_CHECK_STALL_SECONDS == 0 {
                    info!("[CLIPBOARD] HEALTH CHECK: no successful clipboard poll for {}s ({} consecutive failures). Monitor may be stalled.", seconds_since_successful_poll, *failures);
                }

                if read_failed {
                    *failures += 1;
                    // After 3 consecutive read failures, force-reset dedup state
                    // so the next successful read is always intercepted.
                    if *failures >= 3 {
                        info!("[CLIPBOARD] 3 consecutive read failures — resetting dedup state");
                        *manager.last_clipboard_text.lock().await = None;
                        *last_ts = 0;
                        *failures = 0;
                        *manager.stale_read_count.lock().await = 0;
                    }
                } else {
                    *failures = 0;
                }
                drop(last_ts);
                drop(failures);

                // Skip text processing when monitoring is suppressed (e.g., during paste restore)
                // Also skip if stale-recovery already intercepted new text — prevents double-intercept.
                let suppressed = *manager.suppress_monitoring.lock().unwrap();
                if stale_recovery_intercepted {
                    info!("[CLIPBOARD] Skipping normal processing — stale-recovery already intercepted new text");
                } else if !suppressed && !text.trim().is_empty() && text.len() < 100_000 {
                    match manager.intercept_clipboard_text(&text).await {
                        Ok(Some(id)) => info!("[CLIPBOARD] Intercepted, waiting for user: {}", id),
                        Ok(None) => info!("[CLIPBOARD] Text deduped or empty"),
                        Err(e) => info!("[CLIPBOARD] Intercept error: {}", e),
                    }
                } else if suppressed {
                    info!("[CLIPBOARD] Monitoring suppressed — skipping text intercept");
                }

                // --- IMAGE / OCR TRACKING ---
                // Every 2 seconds, try OCR on any image in clipboard (screenshots from Win+Shift+S)
                if tick % OCR_CHECK_INTERVAL_TICKS == 0 {
                    info!("[OCR] Checking clipboard for image...");
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
                                info!("[OCR] Duplicate text within 10s, skipping");
                            } else {
                                if let Some(id) = manager.intercept_ocr_text(&normalized).await {
                                    info!("[OCR] Intercepted, waiting for user: {}", id);
                                }
                            }
                        }
                        Ok(_) => {
                            info!("[OCR] No text found in clipboard image");
                        }
                        Err(e) => {
                            info!("[OCR] Skipped: {}", e);
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
        if let Some(ref lt) = *last { if lt == &normalized {
            info!("[CLIPBOARD] Dedup — same text as previous intercept, skipping");
            return Ok(None);
        } }
        *last = Some(normalized.clone());
        drop(last);
        *self.last_clipboard_timestamp.lock().await = Utc::now().timestamp();

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

    /// Temporarily suppress clipboard monitoring.
    /// Call this before clipboard-restore operations during paste to prevent
    /// the restored text from triggering a spurious intercept event.
    pub fn set_suppress_monitoring(&self, suppress: bool) {
        let mut guard = self.suppress_monitoring.lock().unwrap();
        *guard = suppress;
        if suppress {
            info!("[CLIPBOARD] Monitoring suppressed (paste in progress)");
        } else {
            info!("[CLIPBOARD] Monitoring resumed");
        }
    }

    pub async fn stop_auto_track(&self) {
        let mut running = self.auto_track_running.lock().await;
        *running = false;
    }

    /// FIX 5: Manual refresh — clears dedup state and forces a re-read of clipboard.
    /// Call this from the UI when the user suspects clipboard items are being missed.
    pub async fn force_refresh_clipboard(&self) -> Result<String> {
        info!("[CLIPBOARD] Manual refresh requested — clearing dedup state and re-reading clipboard");

        // Clear all dedup state
        *self.last_clipboard_text.lock().await = None;
        *self.last_clipboard_timestamp.lock().await = 0;
        *self.stale_read_count.lock().await = 0;
        *self.tauri_empty_count.lock().await = 0;
        *self.consecutive_read_failures.lock().await = 0;

        // Try to read current clipboard and intercept it
        let app_handle = self.app_handle.clone();
        let text = match app_handle.clipboard().read_text() {
            Ok(t) if !t.trim().is_empty() => {
                let normalized = Self::normalize_text(&t);
                info!("[CLIPBOARD] Manual refresh read {} chars from Tauri", normalized.len());
                match self.intercept_clipboard_text(&normalized).await {
                    Ok(Some(id)) => {
                        info!("[CLIPBOARD] Manual refresh intercepted: {}", id);
                        format!("Intercepted {} chars (id={})", normalized.len(), id)
                    }
                    Ok(None) => {
                        info!("[CLIPBOARD] Manual refresh: text deduped or empty after normalize");
                        "Clipboard text deduped (same as recent)".to_string()
                    }
                    Err(e) => {
                        info!("[CLIPBOARD] Manual refresh intercept error: {}", e);
                        format!("Error: {}", e)
                    }
                }
            }
            Ok(_) => {
                info!("[CLIPBOARD] Manual refresh: clipboard empty via Tauri, trying PowerShell");
                #[cfg(windows)]
                {
                    match self.read_clipboard_text_powershell() {
                        Ok(ps_text) if !ps_text.trim().is_empty() => {
                            let normalized = Self::normalize_text(&ps_text);
                            info!("[CLIPBOARD] Manual refresh read {} chars from PowerShell", normalized.len());
                            match self.intercept_clipboard_text(&normalized).await {
                                Ok(Some(id)) => format!("Intercepted {} chars via PowerShell (id={})", normalized.len(), id),
                                Ok(None) => "PowerShell text deduped".to_string(),
                                Err(e) => format!("Error: {}", e),
                            }
                        }
                        Ok(_) => "Clipboard is empty".to_string(),
                        Err(e) => format!("PowerShell read failed: {}", e),
                    }
                }
                #[cfg(not(windows))]
                { "Clipboard is empty".to_string() }
            }
            Err(e) => {
                info!("[CLIPBOARD] Manual refresh Tauri read failed: {}", e);
                #[cfg(windows)]
                {
                    match self.read_clipboard_text_powershell() {
                        Ok(ps_text) if !ps_text.trim().is_empty() => {
                            let normalized = Self::normalize_text(&ps_text);
                            match self.intercept_clipboard_text(&normalized).await {
                                Ok(Some(id)) => format!("Intercepted {} chars via PowerShell fallback (id={})", normalized.len(), id),
                                Ok(None) => "PowerShell text deduped".to_string(),
                                Err(e2) => format!("Error: {}", e2),
                            }
                        }
                        Ok(_) => "Clipboard is empty".to_string(),
                        Err(e2) => format!("Both Tauri and PowerShell failed: {} / {}", e, e2),
                    }
                }
                #[cfg(not(windows))]
                { format!("Clipboard read failed: {}", e) }
            }
        };

        Ok(text)
    }
}
