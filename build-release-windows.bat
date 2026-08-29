@echo off
setlocal EnableExtensions
cd /d "%~dp0"
taskkill /f /im RemoteEnvCollector.exe /t
echo [1/4] Checking build tools...
where cargo >nul 2>nul
if errorlevel 1 goto missing_cargo
where npm >nul 2>nul
if errorlevel 1 goto missing_npm
if not exist "app\ui\node_modules\.bin\tauri.cmd" goto missing_tauri

set "LOG=%CD%\release-build.log"
echo [2/4] Cleaning Rust release cache...
cargo clean >> "%LOG%" 2>&1
if errorlevel 1 (echo ERROR: cargo clean failed.& goto :fail)

echo [3/4] Building Windows Release. Log: release-build.log
powershell -NoProfile -ExecutionPolicy Bypass -Command "& '%CD%\scripts\build-release.ps1'" >> "%LOG%" 2>&1
if errorlevel 1 (echo ERROR: Release build failed. See release-build.log.& goto :fail)

echo [4/4] Verifying artifacts...
set "EXE=%CD%\Release\Windows\RemoteEnvCollector\RemoteEnvCollector.exe"
set "SETUP=%CD%\Release\Windows\RemoteEnvCollector-Setup.exe"
if not exist "%EXE%" (echo ERROR: EXE not found.& goto :fail)
if not exist "%SETUP%" (echo ERROR: installer not found.& goto :fail)
for %%F in ("%EXE%" "%SETUP%") do if %%~zF LEQ 0 (echo ERROR: empty artifact %%~F.& goto :fail)

echo.
echo Release build succeeded.
echo EXE: %EXE%
echo Installer: %SETUP%
echo Log: %LOG%
echo.
pause
exit /b 0

:missing_cargo
echo ERROR: cargo was not found.
goto :fail

:missing_npm
echo ERROR: npm was not found.
goto :fail

:missing_tauri
echo ERROR: Tauri CLI was not found. Run npm --prefix app\ui ci first.
goto :fail

:fail
echo.
echo Release build failed. See %CD%\release-build.log
pause
exit /b 1
