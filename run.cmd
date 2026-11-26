@echo off
rem GraphiteCreator launcher (2026-11-20).
rem
rem Usage:
rem   run.cmd              build embedded-assets debug exe, then launch
rem   run.cmd --dev        Tauri dev mode (Vite :1420)
rem   run.cmd --release    build embedded-assets release exe, then launch
rem   run.cmd --build      build NSIS installer via tauri build
rem   run.cmd --test       run the full Rust workspace test suite
rem   run.cmd --launch     launch the existing debug exe without rebuilding
rem   run.cmd --help       show this help
rem
rem Why not plain `cargo build`:
rem   The tauri crate is compiled in dev mode unless the `custom-protocol`
rem   feature is enabled. A dev-mode exe loads build.devUrl
rem   (http://localhost:1420) and shows ERR_CONNECTION_REFUSED when Vite is
rem   not running. `tauri build --no-bundle` enables embedded assets even for
rem   the debug profile, so the exe is self-contained.
rem
rem Notes:
rem   - mingw64 PATH is BUILD-ONLY. WebView2 children inherit PATH, and mingw
rem     DLLs (zlib1.dll) shadow system ones, which crashes the child process.
rem     The launch branches restore ORIG_PATH before starting the exe.
rem   - WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS is the only channel for the
rem     Win11 Insider 26200/26300 sandbox workaround; lib.rs also sets it when
rem     absent. Existing values are never overwritten here.
rem   - The Rust toolchain host is gnu (rust-toolchain.toml). There is NO
rem     .cargo/config.toml with [build] target in this repo, so a plain
rem     cargo/tauri build emits to target\{debug,release} instead of the
rem     triple dir. 2026-11-25 fix: the build branches pass an explicit
rem     --target x86_64-pc-windows-gnu so binaries ALWAYS land under
rem     target\x86_64-pc-windows-gnu\{debug,release}, matching the EXE
rem     paths below regardless of any future cargo config.

setlocal EnableExtensions
cd /d "%~dp0"
set "ROOT=%CD%"
set "TARGET_DIR=%ROOT%\target\x86_64-pc-windows-gnu"
set "MODMAKER_SCHEMAS=%ROOT%\schemas"

rem Prefer a bundled demo-mod; fall back to the workspace sample mod.
set "MODMAKER_DEMO=%ROOT%\demo-mod"
if not exist "%MODMAKER_DEMO%\mod.json" if not exist "%MODMAKER_DEMO%\mod.hjson" (
    if exist "%ROOT%\..\mod\mod.json" set "MODMAKER_DEMO=%ROOT%\..\mod"
)

if not defined WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS (
    rem 2026-11-21: GPU verified working via ANGLE D3D11 on this machine.
    rem Keep --no-sandbox (Win11 Insider sandbox crash); add --disable-gpu only
    rem as a manual fallback if the WebView2 GPU process starts crashing again.
    set "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--no-sandbox"
)

set "ORIG_PATH=%PATH%"
set "MSYS_BIN=C:\msys64\mingw64\bin"
if exist "%MSYS_BIN%\gcc.exe" (
    set "PATH=%MSYS_BIN%;%PATH%"
) else (
    echo [warn] %MSYS_BIN%\gcc.exe not found; Rust linking may fail.
)

where npm >nul 2>nul
if errorlevel 1 (
    echo [error] npm not found. Install Node.js first.
    goto fail
)
where cargo >nul 2>nul
if errorlevel 1 (
    echo [error] cargo not found. Install the Rust toolchain first.
    goto fail
)

if /I "%~1"=="--dev" goto dev
if /I "%~1"=="dev" goto dev
if /I "%~1"=="--release" goto release
if /I "%~1"=="release" goto release
if /I "%~1"=="--build" goto build
if /I "%~1"=="build" goto build
if /I "%~1"=="--test" goto test
if /I "%~1"=="test" goto test
if /I "%~1"=="--launch" goto launch
if /I "%~1"=="launch" goto launch
if /I "%~1"=="--help" goto help
if /I "%~1"=="help" goto help
if not "%~1"=="" (
    echo [error] unknown option: %~1
    goto help
)
goto debug

:ensure_modules
if exist "%ROOT%\node_modules" exit /b 0
echo [info] node_modules missing; running npm install...
call npm install
if errorlevel 1 (
    echo [error] npm install failed.
    exit /b 1
)
exit /b 0

:check_running
tasklist /FI "IMAGENAME eq graphitecreator.exe" 2>nul | find /I "graphitecreator.exe" >nul
if not errorlevel 1 (
    echo [error] graphitecreator.exe is running; close it before rebuilding.
    echo [info] use "run.cmd --launch" to start the existing build instead.
    exit /b 1
)
exit /b 0

:debug
call :ensure_modules
if errorlevel 1 goto fail
call :check_running
if errorlevel 1 goto fail
echo [info] building debug exe with embedded frontend assets...
rem Explicit --target: without it cargo uses the host-default output dir
rem (target\debug) and the EXE path below would point at a stale binary.
call npm run tauri -- build --debug --no-bundle --target x86_64-pc-windows-gnu
if errorlevel 1 (
    echo [error] tauri debug build failed.
    goto fail
)
set "EXE=%TARGET_DIR%\debug\graphitecreator.exe"
if not exist "%EXE%" (
    echo [error] executable not found: %EXE%
    goto fail
)
echo [info] launching %EXE%
set "PATH=%ORIG_PATH%"
start "" /D "%ROOT%" "%EXE%"
exit /b 0

:release
call :ensure_modules
if errorlevel 1 goto fail
call :check_running
if errorlevel 1 goto fail
echo [info] building release exe with embedded frontend assets...
rem Explicit --target: see the debug branch note above.
call npm run tauri -- build --no-bundle --target x86_64-pc-windows-gnu
if errorlevel 1 (
    echo [error] tauri release build failed.
    goto fail
)
set "EXE=%TARGET_DIR%\release\graphitecreator.exe"
if not exist "%EXE%" (
    echo [error] executable not found: %EXE%
    goto fail
)
echo [info] launching %EXE%
set "PATH=%ORIG_PATH%"
start "" /D "%ROOT%" "%EXE%"
exit /b 0

:dev
call :ensure_modules
if errorlevel 1 goto fail
call :check_running
if errorlevel 1 goto fail
echo [info] starting tauri dev (Vite :1420).
echo [warn] dev mode lets WebView2 inherit the mingw PATH. If the window
echo [warn] crashes on startup, use run.cmd without arguments instead.
call npm run tauri dev
set "RC=%errorlevel%"
set "PATH=%ORIG_PATH%"
exit /b %RC%

:build
call :ensure_modules
if errorlevel 1 goto fail
echo [info] building NSIS installer...
call npm run tauri -- build --bundles nsis
set "RC=%errorlevel%"
set "PATH=%ORIG_PATH%"
if not "%RC%"=="0" echo [error] installer build failed with exit code %RC%.
exit /b %RC%

:test
echo [info] running cargo test --workspace...
cargo test --workspace --locked
set "RC=%errorlevel%"
set "PATH=%ORIG_PATH%"
exit /b %RC%

:launch
set "EXE=%TARGET_DIR%\debug\graphitecreator.exe"
if not exist "%EXE%" (
    echo [error] executable not found: %EXE%
    echo [info] run "run.cmd" once to build it.
    goto fail
)
echo [info] launching %EXE%
set "PATH=%ORIG_PATH%"
start "" /D "%ROOT%" "%EXE%"
exit /b 0

:help
echo.
echo GraphiteCreator launcher
echo.
echo   run.cmd              build embedded-assets debug exe, then launch
echo   run.cmd --dev        Tauri dev mode (Vite :1420)
echo   run.cmd --release    build embedded-assets release exe, then launch
echo   run.cmd --build      build NSIS installer via tauri build
echo   run.cmd --test       run the full Rust workspace test suite
echo   run.cmd --launch     launch the existing debug exe without rebuilding
echo   run.cmd --help       show this help
echo.
exit /b 0

:fail
set "PATH=%ORIG_PATH%"
exit /b 1
