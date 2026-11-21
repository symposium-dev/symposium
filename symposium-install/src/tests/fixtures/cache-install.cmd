@echo off
setlocal
set "fixture=%~dp0"
echo %1>> "%fixture%attempts"
if not exist "%fixture%succeed-on" goto failure
set /p succeed=<"%fixture%succeed-on"
if "%succeed%"=="both" goto success
if not "%~1"=="%succeed%" goto failure
:success
if exist "%fixture%omit-binary" exit /b 0
:arguments
if "%~1"=="" goto failure
if "%~1"=="--root" goto install
shift
goto arguments
:install
shift
if not exist "%~1\bin" mkdir "%~1\bin"
copy /y "%fixture%payload.exe" "%~1\bin\cached-tool.exe" >nul
exit /b %errorlevel%
:failure
echo simulated replacement failure 1>&2
exit /b 23
