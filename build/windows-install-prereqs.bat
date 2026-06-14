@echo off
setlocal enabledelayedexpansion
title TurboTalk — Install Prerequisites

set PATH=C:\Program Files\Git\bin;C:\Program Files\Git\cmd;%PATH%

echo.
echo ============================================
echo  TurboTalk — Install Prerequisites
echo ============================================
echo.
echo This will install:
echo   1. Git for Windows  -- https://git-scm.com
echo   2. Node.js 22        -- https://nodejs.org
echo   3. Rust (rustup)     -- https://rustup.rs
echo   4. NSIS              -- https://nsis.sourceforge.io
echo.
echo Each install downloads ~100-500 MB. Allow 15-20 minutes.
echo.

where winget >nul 2>nul
if !ERRORLEVEL! neq 0 (
    echo ERROR: winget not found.
    echo   Install manually from the links above.
    pause
    exit /b 1
)
echo [OK] winget found.
echo.

REM ── Step 1: Git for Windows ─────────────────────────────────────────────
echo Step 1 of 4: Git for Windows
where git >nul 2>nul
if !ERRORLEVEL! equ 0 (
    echo   [SKIP] Already installed.
) else (
    winget install --id Git.Git --exact --silent --accept-package-agreements --accept-source-agreements
    if !ERRORLEVEL! neq 0 ( echo   [WARN] Install Git manually: https://git-scm.com/download/win ) else ( echo   [OK] Installed. )
)
echo.

REM ── Step 2: Node.js 22 ─────────────────────────────────────────────────
echo Step 2 of 4: Node.js 22
where node >nul 2>nul
if !ERRORLEVEL! equ 0 (
    for /f "tokens=*" %%i in ('node --version') do set NODE_VER=%%i
    echo   [SKIP] Already installed: !NODE_VER!
) else (
    winget install --id OpenJS.NodeJS.LTS --exact --silent --accept-package-agreements --accept-source-agreements
    if !ERRORLEVEL! neq 0 ( echo   [WARN] Install Node.js manually: https://nodejs.org/ ) else ( echo   [OK] Installed. )
)
echo.

REM ── Step 3: Rust ───────────────────────────────────────────────────────
echo Step 3 of 4: Rust
where rustc >nul 2>nul
if !ERRORLEVEL! equ 0 (
    for /f "tokens=*" %%i in ('rustc --version') do set RUST_VER=%%i
    echo   [SKIP] Already installed: !RUST_VER!
) else (
    echo   Downloading rustup...
    curl -fsSL -o "%TEMP%\rustup-init.exe" https://win.rustup.rs/x86_64
    "%TEMP%\rustup-init.exe" -y --profile default 2>nul
    if !ERRORLEVEL! neq 0 ( echo   [WARN] Install Rust manually: https://rustup.rs/ ) else ( echo   [OK] Rust installed. Restart may be needed. )
)
echo.

REM ── Step 4: NSIS ───────────────────────────────────────────────────────
echo Step 4 of 4: NSIS
where makensis >nul 2>nul
if !ERRORLEVEL! equ 0 (
    echo   [SKIP] Already installed.
) else (
    winget install --id NSIS.NSIS --exact --silent --accept-package-agreements --accept-source-agreements
    if !ERRORLEVEL! neq 0 ( echo   [WARN] Install NSIS manually: https://nsis.sourceforge.io/ ) else ( echo   [OK] Installed. )
)
echo.

echo ============================================
echo  All done! Restart your computer.
echo ============================================
echo.
pause
