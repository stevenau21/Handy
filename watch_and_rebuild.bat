@echo off
chcp 65001 >nul
echo ============================================
echo  Handy Auto-Rebuild + Auto-Restart Watcher
echo ============================================
echo.
echo  This script will:
echo   1. Watch src-tauri/src/**/*.rs for changes
echo   2. Auto-rebuild the release binary on change
echo   3. Kill the old handy.exe and restart with new one
echo.
echo  Press Ctrl+C to stop watching.
echo.

REM Ensure cargo-watch is in PATH
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

REM Verify cargo-watch is available
cargo watch --version >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo-watch not found. Installing now...
    cargo install cargo-watch
    if errorlevel 1 (
        echo [ERROR] Failed to install cargo-watch.
        pause
        exit /b 1
    )
)

echo [OK] cargo-watch is ready.
echo [INFO] Starting watcher...
echo.

REM Watch Rust source files and rebuild + restart on change
REM -w = watch path
REM -x = execute command on change
REM --postpone = wait for changes to settle before rebuilding
REM The command chain: build release → kill old → start new
cargo watch -w src-tauri/src -x "build --release --features tauri/custom-protocol --manifest-path src-tauri/Cargo.toml" -s "powershell -ExecutionPolicy Bypass -Command \"& { Write-Host '[RESTART] Build complete. Restarting Handy...' -ForegroundColor Green; Stop-Process -Name handy -Force -ErrorAction SilentlyContinue; Start-Sleep 1; Start-Process 'src-tauri\target\release\handy.exe' -WindowStyle Hidden; Write-Host '[RESTART] Handy restarted.' -ForegroundColor Green }\"" --postpone
