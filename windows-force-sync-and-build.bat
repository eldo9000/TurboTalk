@echo off
setlocal
cd /d "%~dp0"

echo.
echo ============================================
echo  TurboTalk — Force Sync and Build Windows
echo ============================================
echo.
echo WARNING: Deletes all local changes.
echo.
pause
echo.

REM ── Step 1: Force-sync from GitHub ──────────────────────────────────────
echo Step 1: Syncing with GitHub...
echo.

git config core.autocrlf false

git fetch origin
if errorlevel 1 (
  echo FETCH FAILED
  pause
  exit /b 1
)

git reset --hard origin/main
if errorlevel 1 (
  echo RESET FAILED
  pause
  exit /b 1
)

git clean -fd
echo Sync complete.
echo.

REM ── Step 2: Check prerequisites ──────────────────────────────────────────
echo Step 2: Checking prerequisites...
echo.

where git >nul 2>nul
if errorlevel 1 (
  echo [FAIL] Git not found. Install from https://git-scm.com/
  pause
  exit /b 1
)

where node >nul 2>nul
if errorlevel 1 (
  echo [FAIL] Node.js 22+ not found. Install from https://nodejs.org/
  pause
  exit /b 1
)

where rustc >nul 2>nul
if errorlevel 1 (
  echo [FAIL] Rust not found. Install from https://rustup.rs/
  pause
  exit /b 1
)

where makensis >nul 2>nul
if errorlevel 1 (
  echo [WARN] NSIS (makensis) not found — installer may not be created.
  echo  Install from https://nsis.sourceforge.io/
)

for /f "tokens=2" %%v in ('node --version') do set NODE_VER=%%v
for /f "tokens=2" %%v in ('rustc --version') do set RUST_VER=%%v
echo  Node: %NODE_VER%
echo  Rust: %RUST_VER%
echo.

REM ── Step 3: Build ────────────────────────────────────────────────────────
echo Step 3: Building TurboTalk...
echo  (First build downloads crates, whisper models, ONNX Runtime)
echo.

echo  [1/4] Installing npm dependencies...
call npm install
if errorlevel 1 (
  echo [FAIL] npm install failed
  pause
  exit /b 1
)
echo.

echo  [2/4] Fetching whisper sidecars...
call npm run fetch-sidecars
if errorlevel 1 (
  echo [FAIL] fetch-sidecars failed
  pause
  exit /b 1
)
echo.

echo  [3/4] Fetching ONNX Runtime...
call npm run fetch-onnxruntime
if errorlevel 1 (
  echo [FAIL] fetch-onnxruntime failed
  pause
  exit /b 1
)
echo.

echo  [4/4] Building TurboTalk (tauri build)...
call npm run package
if errorlevel 1 (
  echo [FAIL] Build failed
  pause
  exit /b 1
)
echo.

REM ── Done ─────────────────────────────────────────────────────────────────
echo ============================================
echo  BUILD COMPLETE!
echo ============================================
echo.

dir /b "dist-artifacts\*.exe" 2>nul
if errorlevel 1 (
  echo Check dist-artifacts\ for output files
) else (
  echo  Installer: %CD%\dist-artifacts\
)
echo.
pause
