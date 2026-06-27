@echo off
REM ---------------------------------------------------------------------------
REM Build the standalone sidecar binary for distribution (Nuitka onefile),
REM then place it where Tauri externalBin expects it
REM (src-tauri/binaries/alpha-compass-sidecar-<triple>.exe).
REM Run this before "npm run tauri build" whenever the sidecar code changes.
REM ---------------------------------------------------------------------------
setlocal
cd /d "%~dp0"

set "TRIPLE=x86_64-pc-windows-msvc"
set "OUT_DIR=build_nuitka"
set "DEST=..\src-tauri\binaries"

echo [1/3] Ensuring Nuitka is installed...
uv run python -m nuitka --version >nul 2>&1
if errorlevel 1 uv pip install nuitka ordered-set zstandard

echo [2/3] Building sidecar with Nuitka (several minutes)...
uv run python -m nuitka --onefile --assume-yes-for-downloads --windows-console-mode=disable --output-dir=%OUT_DIR% --output-filename=alpha-compass-sidecar.exe --include-package=app --include-package=uvicorn --include-package=yfinance --include-package-data=certifi --include-package-data=tzdata --remove-output server.py
if errorlevel 1 (
    echo [ERROR] Nuitka build failed.
    exit /b 1
)

echo [3/3] Copying to %DEST% ...
if not exist "%DEST%" mkdir "%DEST%"
copy /Y "%OUT_DIR%\alpha-compass-sidecar.exe" "%DEST%\alpha-compass-sidecar-%TRIPLE%.exe"

echo Done. Now run:  npm run tauri build
endlocal
