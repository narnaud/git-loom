@echo off
rem Runs coverage.sh under Git Bash, passing arguments through. A bare `bash`
rem on Windows is often WSL's, which runs the script against a Linux toolchain.
setlocal
set "GIT_EXE="
for %%I in (git.exe) do set "GIT_EXE=%%~$PATH:I"
if not defined GIT_EXE (
    echo git.exe is not on PATH; Git for Windows provides the bash this needs.
    exit /b 1
)
for %%I in ("%GIT_EXE%\..\..") do set "GIT_ROOT=%%~fI"
set "GIT_BASH=%GIT_ROOT%\bin\bash.exe"
if not exist "%GIT_BASH%" (
    echo Git Bash not found at "%GIT_BASH%".
    exit /b 1
)
"%GIT_BASH%" "%~dp0coverage.sh" %*
exit /b %ERRORLEVEL%
