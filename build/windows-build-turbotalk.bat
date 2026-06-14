@echo off
setlocal enabledelayedexpansion
title TurboTalk — Build Windows Installer

set PATH=C:\Program Files\Git\bin;C:\Program Files\Git\cmd;C:\Program Files\nodejs;%USERPROFILE%\.cargo\bin;%USERPROFILE%\AppData\Local\Programs\NSIS;%PATH%

echo.
echo ============================================
echo  TurboTalk — Build Windows Installer
echo ============================================
echo.

REM ── Check prerequisites ────────────────────────────────────────────────
where git >nul 2>nul || ( echo [FAIL] Git not found. Run windows-install-prereqs.bat & pause & exit /b 1 )
where node >nul 2>nul || ( echo [FAIL] Node.js not found. Run windows-install-prereqs.bat & pause & exit /b 1 )
where rustc >nul 2>nul || ( echo [FAIL] Rust not found. Run windows-install-prereqs.bat & pause & exit /b 1 )
where makensis >nul 2>nul || ( echo [WARN] NSIS not found. Install from https://nsis.sourceforge.io/ & echo Continuing — build may fail. )

for /f "tokens=2" %%v in ('node --version') do set NODE_VER=%%v
for /f "tokens=2" %%v in ('rustc --version') do set RUST_VER=%%v
echo  Node:  !NODE_VER!
echo  Rust:  !RUST_VER!
echo.

REM ── Step 1: Clone or update the repo ────────────────────────────────────
if not exist "turbotalk" (
    echo Step 1: Cloning repository...
    git clone --config core.autocrlf=false https://github.com/eldo9000/TurboTalk.git turbotalk
    if !ERRORLEVEL! neq 0 ( echo Clone failed. & pause & exit /b )
    cd turbotalk
    echo [OK] Cloned.
) else (
    echo Step 1: Updating repository...
    cd turbotalk
    git pull --ff-only
    if !ERRORLEVEL! neq 0 ( echo Pull failed. & pause & exit /b )
    echo [OK] Updated.
)
echo.

REM ── Step 2: Build ───────────────────────────────────────────────────────
echo Step 2: Building TurboTalk...
echo (Downloads crates, sidecars, ONNX Runtime on first build.)
echo.

call npm install
if !ERRORLEVEL! neq 0 ( echo [FAIL] npm install failed & pause & exit /b )
echo.

echo [2/5] Fetching whisper sidecars...
call npm run fetch-sidecars
if !ERRORLEVEL! neq 0 ( echo [FAIL] fetch-sidecars failed & pause & exit /b )
echo.

echo [3/5] Fetching ONNX Runtime...
call npm run fetch-onnxruntime
if !ERRORLEVEL! neq 0 ( echo [FAIL] fetch-onnxruntime failed & pause & exit /b )
echo.

echo [4/5] Building TurboTalk...
call npm run package
if !ERRORLEVEL! neq 0 ( echo [FAIL] Build failed. & pause & exit /b )
echo.

REM ── Done ────────────────────────────────────────────────────────────────
echo ============================================
echo  BUILD COMPLETE!
echo ============================================
echo.
dir /b "dist-artifacts\*.exe" 2>nul && (
    echo  Installer: %CD%\dist-artifacts\
) || (
    echo  Check %CD%\dist-artifacts\ for output
)
echo.
pause
