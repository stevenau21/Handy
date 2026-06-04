@echo off
REM Quick release build: skips NSIS bundling (we only need handy.exe for the desktop shortcut).
REM Logs to build_release_quick.log and exits when done.

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
set VULKAN_SDK=C:\VulkanSDK\1.4.304.1
set LIBCLANG_PATH=C:\PROGRA~1\LLVM\bin
set PATH=C:\PROGRA~1\CMake\bin;C:\PROGRA~2\MICROS~2\2022\BUILDT~1\VC\Tools\MSVC\1444~1.352\bin\Hostx64\x64;%PATH%
set CMAKE_C_COMPILER=C:\PROGRA~2\MICROS~2\2022\BUILDT~1\VC\Tools\MSVC\1444~1.352\bin\Hostx64\x64\cl.exe
set CMAKE_CXX_COMPILER=C:\PROGRA~2\MICROS~2\2022\BUILDT~1\VC\Tools\MSVC\1444~1.352\bin\Hostx64\x64\cl.exe
set CC=%CMAKE_C_COMPILER%
set CXX=%CMAKE_CXX_COMPILER%
cd /d F:\projects\Handy\src-tauri
cargo build --release > F:\projects\Handy\build_release_quick.log 2>&1
echo EXIT_CODE=%ERRORLEVEL% >> F:\projects\Handy\build_release_quick.log
