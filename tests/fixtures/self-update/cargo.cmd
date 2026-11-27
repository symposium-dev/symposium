@echo off
if "%~1"=="search" (
    echo symposium = "99.0.0"
    exit /b 0
)
if not "%~1"=="install" exit /b 2
if not "%~2"=="symposium" exit /b 2
if not "%~3"=="--force" exit /b 2
"%SELF_UPDATE_TEST_CARGO%" install --path "%SELF_UPDATE_TEST_PACKAGE%" --root "%SELF_UPDATE_TEST_ROOT%" --target-dir "%SELF_UPDATE_TEST_TARGET%" --debug --offline --locked --force
exit /b %errorlevel%
