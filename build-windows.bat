@echo off
REM TurboTalk — Windows Build Script
REM Double-click to build the Windows installer from the repo folder.
REM Requires: Git, Node.js 22, Rust (rustup), NSIS
REM
REM Usage:
REM   First run:  Requires internet — downloads crates, sidecars, ONNX Runtime
REM   Re-runs:    Fast incremental build; only recompiles what changed

setlocal enabledelayedexpansion

title TurboTalk Build (Windows)

echo ============================================
echo  TurboTalk Windows Build
echo ============================================
echo.

:: Check prerequisites
where git >nul 2>nul || ( echo [FAIL] Git not found. Install from https://git-scm.com/ & pause & exit /b 1 )
where node >nul 2>nul || ( echo [FAIL] Node.js not found. Install from https://nodejs.org/ ^(v22+^) & pause & exit /b 1 )
where rustc >nul 2>nul || ( echo [FAIL] Rust not found. Install from https://rustup.rs/ & pause & exit /b 1 )
where makensis >nul 2>nul || ( echo [WARN] NSIS (makensis) not found. Install from https://nsis.sourceforge.io/ & pause & exit /b 1 )

for /f "tokens=2" %%v in ('node --version') do set NODE_VER=%%v
for /f "tokens=2" %%v in ('rustc --version') do set RUST_VER=%%v
echo  Node:  !NODE_VER!
echo  Rust:  !RUST_VER!
echo  Repo:  %CD%
echo.

:: Ensure we're in the repo root
if not exist "src-tauri\Cargo.toml" (
    echo [FAIL] Cargo.toml not found. Make sure you're in the TurboTalk repo root.
    pause
    exit /b 1
)

:: Step 1 — Install npm deps
echo [1/5] Installing npm dependencies...
call npm install
if %errorlevel% neq 0 ( echo [FAIL] npm install failed & pause & exit /b 1 )
echo.
echo [2/5] Fetching whisper sidecars...
call npm run fetch-sidecars
if %errorlevel% neq 0 ( echo [FAIL] fetch-sidecars failed & pause & exit /b 1 )
echo.
echo [3/5] Fetching ONNX Runtime...
call npm run fetch-onnxruntime
if %errorlevel% neq 0 ( echo [FAIL] fetch-onnxruntime failed & pause & exit /b 1 )
echo.
echo [4/5] Building TurboTalk...
call npm run package
if %errorlevel% neq 0 ( echo [FAIL] build failed. Check output above. & pause & exit /b 1 )
echo.
echo [5/5] Collecting artifacts...
if exist "dist-artifacts" (
    dir /b "dist-artifacts\*.exe" 2>nul && (
        echo.
        echo ============================================
        echo  Build complete!
        echo  Installer: %CD%\dist-artifacts\
        echo ============================================
    ) || (
        echo [WARN] dist-artifacts folder is empty. Check build logs.
    )
) else (
    echo [WARN] dist-artifacts folder not found.
)
echo.
echo Done.
pause
