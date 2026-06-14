#!/usr/bin/env bash
# TurboTalk — macOS Build Script
# Double-click or run from terminal to build the DMG.
# Requires: Node.js 22, Rust, Homebrew + whisper-cpp

set -euo pipefail

cd "$(dirname "$0")"

echo "============================================"
echo " TurboTalk macOS Build"
echo "============================================"
echo ""

# Quick prereq checks
command -v node  >/dev/null 2>&1 || { echo "[FAIL] Node.js not found — install from https://nodejs.org/"; exit 1; }
command -v rustc >/dev/null 2>&1 || { echo "[FAIL] Rust not found — run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; exit 1; }
command -v brew  >/dev/null 2>&1 || echo "[WARN] Homebrew not found — whisper-server may be missing"

echo " Node: $(node --version)"
echo " Rust: $(rustc --version)"
echo " Repo: $PWD"
echo ""

# Ensure whisper-server is available
if ! command -v whisper-server &>/dev/null; then
    echo "[1/5] Installing whisper.cpp via Homebrew..."
    brew install whisper-cpp
fi

echo "[1/5] Installing npm dependencies..."
npm install

echo "[2/5] Fetching VAD model..."
npm run fetch-vad-model

echo "[3/5] Generating icons..."
npm run gen-icons

echo "[4/5] Building TurboTalk DMG..."
npm run tauri build

echo "[5/5] Collecting artifacts..."
mkdir -p dist-artifacts
cp target/release/bundle/dmg/*.dmg dist-artifacts/ 2>/dev/null || true

if ls dist-artifacts/*.dmg 2>/dev/null | head -1 >/dev/null; then
    echo ""
    echo "============================================"
    echo " Build complete!"
    echo " DMG: $PWD/dist-artifacts/"
    echo "============================================"
else
    echo "[WARN] DMG not found in dist-artifacts/"
fi
echo ""
echo "Done."
