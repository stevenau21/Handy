use anyhow::{anyhow, Result};
use log::info;

/// OCR via Windows.Media.Ocr (GPU-accelerated via DirectX).
/// Replaces the old OCR.space cloud API — runs entirely locally with no network latency.
///
/// 1. Saves clipboard image to temp PNG via PowerShell
/// 2. Loads PNG into a SoftwareBitmap via Windows.Graphics.Imaging
/// 3. Runs OcrEngine.RecognizeAsync (GPU-accelerated)
/// 4. Returns extracted text
#[cfg(windows)]
pub async fn ocr_clipboard_image() -> Result<String> {
    use std::os::windows::process::CommandExt;
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;

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

    let mut command = std::process::Command::new("powershell");
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = command
        .args([
            "-Sta",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            save_script,
        ])
        .output()
        .map_err(|e| anyhow!("Failed to save clipboard image: {}", e))?;

    if !output.status.success() {
        return Err(anyhow!("No image in clipboard"));
    }

    let temp_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if temp_path.is_empty() {
        return Err(anyhow!("Failed to save clipboard image"));
    }

    // Step 2-4: Run the Windows OCR on a blocking thread since
    // IAsyncOperation::get() is synchronous (blocking).
    let temp_path_clone = temp_path.clone();
    let text = tokio::task::spawn_blocking(move || -> Result<String> {
        let path_hstring = HSTRING::from(&temp_path_clone);

        // Open file as random-access stream
        let file = windows::Storage::StorageFile::GetFileFromPathAsync(&path_hstring)
            .map_err(|e| anyhow!("GetFileFromPathAsync failed: {}", e))?
            .get()
            .map_err(|e| anyhow!("GetFileFromPathAsync get failed: {}", e))?;

        let stream = file
            .OpenReadAsync()
            .map_err(|e| anyhow!("OpenReadAsync failed: {}", e))?
            .get()
            .map_err(|e| anyhow!("OpenReadAsync get failed: {}", e))?;

        // Decode the PNG into a SoftwareBitmap
        let decoder = BitmapDecoder::CreateAsync(&stream)
            .map_err(|e| anyhow!("BitmapDecoder::CreateAsync failed: {}", e))?
            .get()
            .map_err(|e| anyhow!("BitmapDecoder::CreateAsync get failed: {}", e))?;

        let software_bitmap = decoder
            .GetSoftwareBitmapAsync()
            .map_err(|e| anyhow!("GetSoftwareBitmapAsync failed: {}", e))?
            .get()
            .map_err(|e| anyhow!("GetSoftwareBitmapAsync get failed: {}", e))?;

        // Run OCR (GPU-accelerated via DirectX)
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()
            .map_err(|e| anyhow!("OcrEngine::TryCreateFromUserProfileLanguages failed: {}", e))?;

        let ocr_result = engine
            .RecognizeAsync(&software_bitmap)
            .map_err(|e| anyhow!("RecognizeAsync failed: {}", e))?
            .get()
            .map_err(|e| anyhow!("RecognizeAsync get failed: {}", e))?;

        let text = ocr_result
            .Text()
            .map_err(|e| anyhow!("OcrResult::Text failed: {}", e))?
            .to_string();

        Ok(text)
    })
    .await
    .map_err(|e| anyhow!("OCR spawn_blocking failed: {}", e))??;

    // Clean up temp file
    let _ = tokio::fs::remove_file(&temp_path).await;

    if text.trim().is_empty() {
        return Err(anyhow!("No text detected in image"));
    }

    info!(
        "OCR (Windows.Media.Ocr GPU) extracted {} characters",
        text.len()
    );
    Ok(text)
}

/// Non-Windows fallback: keep the old OCR.space cloud API.
/// This is used on macOS and Linux where Windows.Media.Ocr is not available.
#[cfg(not(windows))]
pub async fn ocr_clipboard_image() -> Result<String> {
    // Step 1: save clipboard image to temp file via PowerShell (Windows) or osascript (macOS)
    #[cfg(target_os = "macos")]
    let save_script = r#"
set tempFile to (do shell script "mktemp /tmp/handy_ocr_XXXXXX.png")
use framework "AppKit"
set pb to current application's NSPasteboard's generalPasteboard()
set imgData to pb's dataForType:(current application's NSPasteboardTypePNG)
if imgData is missing value then error "No image in clipboard"
imgData's writeToFile:tempFile atomically:true
return tempFile
"#;

    #[cfg(target_os = "macos")]
    let (shell, shell_args) = ("osascript", vec!["-e", save_script]);

    #[cfg(not(target_os = "macos"))]
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

    #[cfg(not(target_os = "macos"))]
    let (shell, shell_args) = ("powershell", vec!["-Sta", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", save_script]);

    let mut command = std::process::Command::new(shell);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = command
        .args(&shell_args)
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
    let image_bytes = tokio::fs::read(&temp_path)
        .await
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

    let json: serde_json::Value = resp
        .json()
        .await
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

    info!("OCR (cloud fallback) extracted {} characters", text.len());
    Ok(text)
}
