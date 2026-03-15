@echo off
setlocal

set PROFILE=%~1
if "%PROFILE%"=="" set PROFILE=debug

if "%PROFILE%"=="debug" (
    cargo build
    if errorlevel 1 exit /b 1
    set GL_BIN=%~dp0target\debug\git-loom.exe
) else if "%PROFILE%"=="release" (
    cargo build --release
    if errorlevel 1 exit /b 1
    set GL_BIN=%~dp0target\release\git-loom.exe
) else (
    echo Usage: test.cmd [debug^|release]
    exit /b 1
)

:: Derive bash path from git: C:\…\Git\cmd\git.exe -> C:\…\Git\bin\bash.exe
for /f "tokens=* usebackq" %%i in (`where git 2^>nul`) do (
    set GIT_EXE=%%i
    goto :found_git
)
echo Error: git not found in PATH
exit /b 1

:found_git
set BASH_EXE=%GIT_EXE:\cmd\git.exe=\bin\bash.exe%
if not exist "%BASH_EXE%" (
    echo Error: bash not found at %BASH_EXE%
    exit /b 1
)

"%BASH_EXE%" "%~dp0tests\integration\run_all.sh"
