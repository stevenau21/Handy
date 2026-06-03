# Guide: Adding Apps to Handy Voice Commands

## The Simple Way (Most Apps Work)

In **Settings → Commands**, add a new command:

| Field | Value |
|---|---|
| Phrase | `open spotify` |
| Action | `Open App` |
| Payload | `Spotify` (or whatever the app appears as in Start Menu) |

This uses Windows `ShellExecuteW` which works for:
- Apps in your Start Menu (`Spotify`, `Discord`, `Code`, `Steam`)
- Simple executable names (`notepad`, `calc`, `cmd`)
- Full paths with or without quotes

## The Exact Way (.lnk Shortcuts or Specific Paths)

For apps that don't launch by name (like Telegram Desktop):

1. **Find the shortcut path:**
   - Open Start Menu, find the app
   - Right-click → **Open file location**
   - Right-click the shortcut → **Properties**
   - Copy the full path from the "Target" field
   - Or use this command in CMD: `dir /s /b "%APPDATA%\Microsoft\Windows\Start Menu" | findstr /i "AppName"`

2. **Use it in Handy:**
   | Phrase | Action | Payload |
   |---|---|---|
   | `open telegram` | Open App | `C:\Users\steve\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Telegram Desktop\Telegram.lnk` |

   Or with **Run Script**:
   | Phrase | Action | Payload |
   |---|---|---|
   | `open telegram` | Run Script | `C:\Users\steve\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Telegram Desktop\Telegram.lnk` |

## Opening to a Specific Chat / User

Use **Open URL** with a URL scheme the app has registered:

| App | URL Scheme | Example Payload |
|---|---|---|
| Telegram | `tg://` | `tg://resolve?domain=stevenau21` |
| Discord | `discord://` | `discord://-/users/@me` |
| Zoom | `zoommtg://` | `zoommtg://zoom.us/join?confno=123456789` |
| Spotify | `spotify://` | `spotify://` |

## Running Custom Scripts

Use **Run Script** with:
- A `.bat` file path: `C:\Users\steve\scripts\my-app.bat`
- A PowerShell command: `powershell -c Start-Process "Spotify"`
- A direct `.exe` path: `C:\Program Files\Something\app.exe`

## The Technical Fix (Why This Now Works)

The original `open_app` handler used `cmd /c start "" "name"`. This broke on paths with spaces because Rust's `Command::args` API passes each argument separately to `cmd.exe`, which corrupts the quoting.

The fix replaces this with the **Windows Shell API** (`ShellExecuteW`) — the same function Windows Explorer itself uses when you double-click a file. It's added to Handy's `src-tauri/src/managers/commands.rs`:

```rust
fn open_with_shell_execute(path: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    // ... calls ShellExecuteW with the path directly
    // No cmd.exe involved, no quoting issues
}
```

And `Win32_UI_Shell` was added to the `windows` crate features in `Cargo.toml`.