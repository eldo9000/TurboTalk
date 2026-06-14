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
echo   1. Git for Windows     -- https://git-scm.com
echo   2. Node.js 22          -- https://nodejs.org
echo   3. Rust (rustup)       -- https://rustup.rs
echo   4. NSIS                -- https://nsis.sourceforge.io
echo   5. 7-Zip               -- https://7-zip.org (needed for ONNX Runtime)
echo   6. Python 3            -- https://python.org (needed for ONNX Runtime)
echo.
echo Each install downloads ~100-500 MB. Allow 15-20 minutes.
echo.

where winget >nul 2>nul
if !ERRORLEVEL! neq 0 (
    echo ERROR: winget not found. Install manually from the links above.
    pause
    exit /b 1
)
echo [OK] winget found.
echo.

REM ── Step 1: Git ─────────────────────────────────────────────────────────
echo Step 1 of 6: Git for Windows
where git >nul 2>nul
if !ERRORLEVEL! equ 0 ( echo   [SKIP] Already installed. ) else (
    winget install --id Git.Git --exact --silent --accept-package-agreements --accept-source-agreements >nul
    if !ERRORLEVEL! neq 0 ( echo   [FAIL] Install manually: https://git-scm.com/download/win ) else ( echo   [OK] Installed. )
)
echo.

REM ── Step 2: Node.js 22 ─────────────────────────────────────────────────
echo Step 2 of 6: Node.js 22
where node >nul 2>nul
if !ERRORLEVEL! equ 0 (
    for /f "tokens=*" %%i in ('node --version') do echo   [SKIP] Already installed: %%i
) else (
    winget install --id OpenJS.NodeJS.LTS --exact --silent --accept-package-agreements --accept-source-agreements >nul
    if !ERRORLEVEL! neq 0 ( echo   [FAIL] Install manually: https://nodejs.org/ ) else ( echo   [OK] Installed. )
)
echo.

REM ── Step 3: Rust ────────────────────────────────────────────────────────
echo Step 3 of 6: Rust
where rustc >nul 2>nul
if !ERRORLEVEL! equ 0 (
    for /f "tokens=*" %%i in ('rustc --version') do echo   [SKIP] Already installed: %%i
) else (
    echo   Downloading rustup...
    curl -fsSL -o "%TEMP%\rustup-init.exe" https://win.rustup.rs/x86_64
    "%TEMP%\rustup-init.exe" -y --profile default 2>nul
    if !ERRORLEVEL! neq 0 ( echo   [FAIL] Install manually: https://rustup.rs/ ) else ( echo   [OK] Rust installed. )
    set PATH=%USERPROFILE%\.cargo\bin;%PATH%
)
echo.

REM ── Step 4: NSIS ────────────────────────────────────────────────────────
echo Step 4 of 6: NSIS
where makensis >nul 2>nul
if !ERRORLEVEL! equ 0 ( echo   [SKIP] Already installed. ) else (
    winget install --id NSIS.NSIS --exact --silent --accept-package-agreements --accept-source-agreements >nul
    if !ERRORLEVEL! neq 0 ( echo   [FAIL] Install manually: https://nsis.sourceforge.io/ ) else ( echo   [OK] Installed. )
)
echo.

REM ── Step 5: 7-Zip ──────────────────────────────────────────────────────
echo Step 5 of 6: 7-Zip
where 7z >nul 2>nul
if !ERRORLEVEL! equ 0 ( echo   [SKIP] Already installed. ) else (
    winget install --id 7zip.7zip --exact --silent --accept-package-agreements --accept-source-agreements >nul
    if !ERRORLEVEL! neq 0 ( echo   [FAIL] Install manually: https://7-zip.org/ ) else ( echo   [OK] Installed. )
)
echo.

REM ── Step 6: Python 3 ────────────────────────────────────────────────────
echo Step 6 of 6: Python 3
where python >nul 2>nul
if !ERRORLEVEL! equ 0 (
    for /f "tokens=2" %%v in ('python --version') do echo   [SKIP] Already installed: %%v
) else (
    winget install --id Python.Python.3.13 --exact --silent --accept-package-agreements --accept-source-agreements >nul
    if !ERRORLEVEL! neq 0 ( echo   [FAIL] Install manually: https://python.org/ ) else ( echo   [OK] Installed. )
)
echo.

echo ============================================
echo  All done! Restart your computer.
echo ============================================
echo.
pause
