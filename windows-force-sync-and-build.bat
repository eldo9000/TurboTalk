@echo off
setlocal
cd /d "%~dp0"

echo.
echo ============================================
echo  TurboTalk - Force Sync and Build Windows
echo ============================================
echo.
echo WARNING: Deletes all local changes.
echo.
pause
echo.

REM Step 1: Force-sync from GitHub
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

REM Step 2: Check prerequisites
echo Step 2: Checking prerequisites...
echo.

where git >nul 2>&1
if errorlevel 1 (
  echo Git not found. Install from https://git-scm.com/
  pause
  exit /b 1
)
echo  [OK] Git

where node >nul 2>&1
if errorlevel 1 (
  echo Node.js 22+ not found. Install from https://nodejs.org/
  pause
  exit /b 1
)
echo  [OK] Node.js

where rustc >nul 2>&1
if errorlevel 1 (
  echo Rust not found. Install from https://rustup.rs/
  pause
  exit /b 1
)
echo  [OK] Rust

where makensis >nul 2>&1
if errorlevel 1 (
  echo  [WARN] NSIS not found - installer may not be created
)

echo.

REM Step 3: Build
echo Step 3: Building TurboTalk...
echo.

echo  [1/6] npm install...
call npm install
if errorlevel 1 (
  echo npm install failed
  pause
  exit /b 1
)
echo.

echo  [2/6] Fetching whisper sidecars...
call npm run fetch-sidecars
if errorlevel 1 (
  echo fetch-sidecars failed
  pause
  exit /b 1
)
echo.

echo  [3/6] Fetching ONNX Runtime...
call npm run fetch-onnxruntime
if errorlevel 1 (
  echo fetch-onnxruntime failed
  pause
  exit /b 1
)
echo.

echo  [4/6] Fetching VAD model...
call npm run fetch-vad-model
if errorlevel 1 (
  echo fetch-vad-model failed
  pause
  exit /b 1
)
echo.

echo  [5/6] Running preflight check...
call npm run preflight
if errorlevel 1 (
  echo preflight check failed
  pause
  exit /b 1
)
echo.

echo  [6/6] Building TurboTalk (tauri build)...
call npx tauri build
if errorlevel 1 (
  echo Build failed
  pause
  exit /b 1
)
echo.

REM Rename artifact (works on all platforms)
echo  Renaming artifact...
call node scripts/rename-artifact.mjs
if errorlevel 1 (
  echo Artifact rename failed
  pause
  exit /b 1
)
echo.

REM Done
echo ============================================
echo  BUILD COMPLETE
echo ============================================
echo.

dir /b "dist-artifacts\*.exe" 2>nul
if errorlevel 1 (
  echo Check dist-artifacts\ for output files
) else (
  echo Installer: %CD%\dist-artifacts\
)
echo.
pause
