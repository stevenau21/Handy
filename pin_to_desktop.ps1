# Drops a desktop shortcut to the release handy.exe.
# Run this AFTER build_release.bat has finished successfully.

$ErrorActionPreference = "Stop"

$ProjectRoot  = "F:\projects\Handy"
$ExePath      = Join-Path $ProjectRoot "src-tauri\target\release\handy.exe"
$WorkingDir   = Join-Path $ProjectRoot "src-tauri\target\release"
$IconPath     = Join-Path $ProjectRoot "src-tauri\icons\icon.ico"
$ShortcutPath = Join-Path $env:USERPROFILE "Desktop\Handy.lnk"

if (-not (Test-Path $ExePath)) {
    Write-Error "Release exe not found at: $ExePath`nRun build_release.bat first."
    exit 1
}

$WshShell  = New-Object -ComObject WScript.Shell
$Shortcut  = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath       = $ExePath
$Shortcut.WorkingDirectory = $WorkingDir
$Shortcut.IconLocation     = "$IconPath,0"
$Shortcut.Description      = "Handy - Speech to Text"
$Shortcut.WindowStyle      = 7   # minimized launcher, app runs normal
$Shortcut.Save()

Write-Host "Shortcut created: $ShortcutPath"
Write-Host "Target:           $ExePath"
Write-Host "Icon:             $IconPath"
