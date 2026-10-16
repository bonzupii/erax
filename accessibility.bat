@echo off
rem accessibility.bat — apply erax accessibility profiles and build (Windows).
rem
rem Usage:
rem   accessibility.bat <profile>...            apply profiles, then release-build
rem   accessibility.bat --unapply <profile>...  reverse profiles (no build)
rem   accessibility.bat --list                  list available profiles
rem
rem Requires patch(1) (ships with Git for Windows) and cargo on PATH.
setlocal EnableDelayedExpansion
cd /d "%~dp0"

if "%~1"=="" goto usage
if "%~1"=="--list" (
    for %%f in (profiles\*.patch) do echo %%~nf
    exit /b 0
)
if "%~1"=="-h" goto usage
if "%~1"=="--help" goto usage

set APPLY=1
if "%~1"=="--unapply" ( set APPLY=0 & shift )

if "%~1"=="" goto usage

set FLAGS=
:apply_loop
if "%~1"=="" goto after_loop
if "%APPLY%"=="0" goto after_loop
if not exist "profiles\%~1.patch" (
    echo unknown profile: %~1
    exit /b 1
)
call :apply_patch "profiles\%~1.patch" || exit /b 1
if exist "profiles\%~1.flags" (
    for /f "delims=" %%l in (profiles\%~1.flags) do set FLAGS=!FLAGS! %%l
)
shift
goto apply_loop
:after_loop
if "%APPLY%"=="1" (
    cargo build --release %FLAGS% || exit /b 1
    exit /b 0
)

rem unapply: reverse order. Rebuild the arg list reversed first.
set REV=
:rev_loop
if "%~1"=="" goto rev_done
set REV=%~1 !REV!
shift
goto rev_loop
:rev_done
for %%p in (%REV%) do (
    if not exist "profiles\%%p.patch" (
        echo unknown profile: %%p
        exit /b 1
    )
    call :reverse_patch "profiles\%%p.patch" || exit /b 1
)
echo profiles reversed; tree is back to stock, no build run.
exit /b 0

rem apply_patch <patchfile>: apply unless already applied.
rem Mirrors accessibility.sh: forward dry-run wins; reverse dry-run
rem means already applied (no-op); neither means config drift.
:apply_patch
patch -p1 --forward --dry-run --silent < "%~1" >nul 2>&1
if not errorlevel 1 (
    patch -p1 --forward < "%~1" || exit /b 1
    goto :eof
)
patch -p1 -R --dry-run --silent < "%~1" >nul 2>&1
if not errorlevel 1 (
    echo already applied, skipping: %~1 1>&2
    goto :eof
)
echo patch does not apply cleanly (config drift?): %~1 1>&2
exit /b 1

rem reverse_patch <patchfile>: reverse unless already reversed.
:reverse_patch
patch -p1 -R --forward --dry-run --silent < "%~1" >nul 2>&1
if not errorlevel 1 (
    patch -R -p1 --forward < "%~1" || exit /b 1
    goto :eof
)
patch -p1 --forward --dry-run --silent < "%~1" >nul 2>&1
if not errorlevel 1 (
    echo already reversed, skipping: %~1 1>&2
    goto :eof
)
echo patch does not reverse cleanly (config drift?): %~1 1>&2
exit /b 1

:usage
echo usage: accessibility.bat ^<profile^>... ^| accessibility.bat --unapply ^<profile^>... ^| accessibility.bat --list
exit /b 1
