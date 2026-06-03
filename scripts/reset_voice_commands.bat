@echo off
setlocal EnableDelayedExpansion

set "SETTINGS_PATH=%APPDATA%\com.pais.handy\settings_store.json"

if not exist "%SETTINGS_PATH%" (
    echo Settings file not found at %SETTINGS_PATH%
    pause
    exit /b 1
)

node -e "
const fs = require('fs');
const path = process.argv[1];
const raw = fs.readFileSync(path, 'utf8');
const data = JSON.parse(raw);

if (!data.settings) {
    console.error('No settings object found');
    process.exit(1);
}

// Reset voice_commands to a clean default list
data.settings.voice_commands = [
    {
        id: 'cmd_open_telegram',
        phrase: 'open telegram',
        action_type: 'run_script',
        action_payload: 'explorer \"C:\\Users\\steve\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Telegram Desktop\\Telegram.lnk\"',
        enabled: true
    }
];

fs.writeFileSync(path, JSON.stringify(data, null, 2));
console.log('Voice commands reset successfully.');
" "%SETTINGS_PATH%"

echo Done. Restart Handy to use the fixed command.
pause
