use anyhow::{anyhow, Result};
use log::info;

/// OCR via OCR.space free API (no key needed for ~25K req/month).
/// 1. Saves clipboard image to temp PNG via PowerShell
/// 2. Uploads to OCR.space (pure async, no nested runtimes)
/// 3. Returns extracted text
pub async fn ocr_clipboard_image() -> Result<String> {
    // Step 1: save clipboard image to temp file via PowerShell
    let save_script = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
if (-not [System.Windows.Forms.Clipboard]::ContainsImage()) {
    throw "No image in clipboard"
}
$img = [System.Windows.Forms.Clipboard]::GetImage()
$tempFile = [System.IO.Path]::GetTempFileName() + ".png"
$img.Save($tempFile, [System.Drawing.Imaging.ImageFormat]::Png)
$img.Dispose()
Write-Output $tempFile
"#;

    let output = std::process::Command::new("powershell")
        .args(["-Sta", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", save_script])
        .output()
        .map_err(|e| anyhow!("Failed to save clipboard image: {}", e))?;

    if !output.status.success() {
        return Err(anyhow!("No image in clipboard"));
    }

    let temp_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if temp_path.is_empty() {
        return Err(anyhow!("Failed to save clipboard image"));
    }

    // Step 2: read file + upload via async reqwest (no blocking runtime)
    let image_bytes = tokio::fs::read(&temp_path).await
        .map_err(|e| anyhow!("Failed to read temp image: {}", e))?;

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(image_bytes)
                .file_name("screenshot.png")
                .mime_str("image/png")?,
        )
        .text("language", "eng")
        .text("isOverlayRequired", "false")
        .text("filetype", "png");

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.ocr.space/parse/image")
        .header("apikey", "helloworld")
        .multipart(form)
        .send()
        .await
        .map_err(|e| anyhow!("OCR.space request failed: {}", e))?;

    let json: serde_json::Value = resp.json().await
        .map_err(|e| anyhow!("OCR.space JSON parse failed: {}", e))?;

    let text = json["ParsedResults"][0]["ParsedText"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if text.is_empty() {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(anyhow!("No text detected in image"));
    }

    // Clean up temp file
    let _ = tokio::fs::remove_file(&temp_path).await;

    info!("OCR extracted {} characters", text.len());
    Ok(text)
}
