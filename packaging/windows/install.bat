@echo off
setlocal EnableExtensions EnableDelayedExpansion

echo Ferrosonic Windows installer
echo ===========================
echo.

set "DEFAULT_INSTALL_DIR=%LOCALAPPDATA%\Ferrosonic"
set "INSTALL_DIR=%~1"
if "%INSTALL_DIR%"=="" set "INSTALL_DIR=%DEFAULT_INSTALL_DIR%"

set "SCRIPT_DIR=%~dp0"
set "SOURCE_EXE=%SCRIPT_DIR%ferrosonic.exe"
set "DEST_EXE=%INSTALL_DIR%\ferrosonic.exe"
set "SOURCE_ICON=%SCRIPT_DIR%ferrosonic.ico"
set "SOURCE_PNG=%SCRIPT_DIR%ferrosonic.png"
set "DEST_ICON=%INSTALL_DIR%\ferrosonic.ico"
set "SHORTCUT_NAME=Ferrosonic.lnk"
set "START_MENU_DIR=%APPDATA%\Microsoft\Windows\Start Menu\Programs"
set "DESKTOP_DIR=%USERPROFILE%\Desktop"

set "FFMPEG_URL=https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
set "FFMPEG_ROOT=%INSTALL_DIR%\ffmpeg"
set "FFMPEG_BIN=%FFMPEG_ROOT%\bin"
set "FFMPEG_ZIP=%TEMP%\ferrosonic-ffmpeg.zip"
set "FFMPEG_TMP=%TEMP%\ferrosonic-ffmpeg-extract-%RANDOM%-%RANDOM%"

if not exist "%SOURCE_EXE%" (
  echo Error: ferrosonic.exe not found next to this installer.
  echo Put install.bat and ferrosonic.exe in the same folder and run again.
  exit /b 1
)

echo Install directory: "%INSTALL_DIR%"
if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"
if errorlevel 1 (
  echo Error: could not create install directory.
  exit /b 1
)

copy /Y "%SOURCE_EXE%" "%DEST_EXE%" >nul
if errorlevel 1 (
  echo Error: could not copy ferrosonic.exe to "%INSTALL_DIR%".
  exit /b 1
)

if exist "%SOURCE_ICON%" (
  copy /Y "%SOURCE_ICON%" "%DEST_ICON%" >nul
)

if not exist "%DEST_ICON%" if exist "%SOURCE_PNG%" (
  set "ICON_SOURCE_PNG=%SOURCE_PNG%"
  powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$pngPath = [System.IO.Path]::GetFullPath($env:ICON_SOURCE_PNG);" ^
    "$icoPath = [System.IO.Path]::GetFullPath($env:DEST_ICON);" ^
    "$png = [System.IO.File]::ReadAllBytes($pngPath);" ^
    "$stream = [System.IO.File]::Open($icoPath, [System.IO.FileMode]::Create);" ^
    "$writer = New-Object System.IO.BinaryWriter($stream);" ^
    "$writer.Write([UInt16]0);" ^
    "$writer.Write([UInt16]1);" ^
    "$writer.Write([UInt16]1);" ^
    "$writer.Write([Byte]0);" ^
    "$writer.Write([Byte]0);" ^
    "$writer.Write([Byte]0);" ^
    "$writer.Write([Byte]0);" ^
    "$writer.Write([UInt16]1);" ^
    "$writer.Write([UInt16]32);" ^
    "$writer.Write([UInt32]$png.Length);" ^
    "$writer.Write([UInt32]22);" ^
    "$writer.Write($png);" ^
    "$writer.Flush();" ^
    "$writer.Close();" ^
    "$stream.Close();"
  if errorlevel 1 (
    echo Warning: could not generate icon file from ferrosonic.png.
  )
)

call :AddPath "%INSTALL_DIR%"
if errorlevel 1 exit /b 1

where ffmpeg >nul 2>&1
if errorlevel 1 (
  echo ffmpeg not found in PATH. Downloading runtime dependencies...

  if exist "%FFMPEG_TMP%" rmdir /S /Q "%FFMPEG_TMP%"
  mkdir "%FFMPEG_TMP%"

  powershell -NoProfile -ExecutionPolicy Bypass -Command "Invoke-WebRequest -Uri '%FFMPEG_URL%' -OutFile '%FFMPEG_ZIP%'"
  if errorlevel 1 (
    echo Error: ffmpeg download failed.
    exit /b 1
  )

  powershell -NoProfile -ExecutionPolicy Bypass -Command "Expand-Archive -Path '%FFMPEG_ZIP%' -DestinationPath '%FFMPEG_TMP%' -Force"
  if errorlevel 1 (
    echo Error: ffmpeg extraction failed.
    exit /b 1
  )

  set "FFMPEG_BUILD_DIR="
  for /D %%D in ("%FFMPEG_TMP%\ffmpeg-*") do (
    set "FFMPEG_BUILD_DIR=%%~fD"
  )

  if "!FFMPEG_BUILD_DIR!"=="" (
    echo Error: unexpected ffmpeg archive structure.
    exit /b 1
  )

  if not exist "!FFMPEG_BUILD_DIR!\bin\ffmpeg.exe" (
    echo Error: ffmpeg.exe not found after extraction.
    exit /b 1
  )

  if exist "%FFMPEG_ROOT%" rmdir /S /Q "%FFMPEG_ROOT%"
  mkdir "%FFMPEG_BIN%"

  xcopy /Y /Q "!FFMPEG_BUILD_DIR!\bin\*" "%FFMPEG_BIN%\" >nul
  if errorlevel 1 (
    echo Error: could not install ffmpeg binaries.
    exit /b 1
  )

  call :AddPath "%FFMPEG_BIN%"
  if errorlevel 1 exit /b 1

  if exist "%FFMPEG_ZIP%" del /F /Q "%FFMPEG_ZIP%" >nul 2>&1
  if exist "%FFMPEG_TMP%" rmdir /S /Q "%FFMPEG_TMP%"

  echo ffmpeg installed in "%FFMPEG_BIN%"
) else (
  echo ffmpeg already available in PATH.
)

if not exist "%START_MENU_DIR%" mkdir "%START_MENU_DIR%"
call :CreateShortcut "%START_MENU_DIR%\%SHORTCUT_NAME%"
if errorlevel 1 exit /b 1

if exist "%DESKTOP_DIR%" (
  call :CreateShortcut "%DESKTOP_DIR%\%SHORTCUT_NAME%"
  if errorlevel 1 exit /b 1
)

echo.
echo Installation complete.
echo Installed binary: "%DEST_EXE%"
echo Shortcut created in Start Menu and Desktop (if available).
echo Open a new terminal to use updated PATH.
echo Launching Ferrosonic...
start "Ferrosonic" "%DEST_EXE%"
if errorlevel 1 (
  echo Warning: could not launch Ferrosonic automatically.
)
exit /b 0

:AddPath
set "PATH_ENTRY=%~1"
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$target = [System.IO.Path]::GetFullPath($env:PATH_ENTRY);" ^
  "$current = [Environment]::GetEnvironmentVariable('Path', 'User');" ^
  "if ([string]::IsNullOrWhiteSpace($current)) { $parts = @() } else { $parts = $current -split ';' | Where-Object { $_ -and $_.Trim() -ne '' } }" ^
  "$exists = $false; foreach ($p in $parts) { if ($p.TrimEnd('\\') -ieq $target.TrimEnd('\\')) { $exists = $true; break } }" ^
  "if (-not $exists) { $parts += $target; [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User'); Write-Host ('Added to user PATH: ' + $target) } else { Write-Host ('Already in user PATH: ' + $target) }"
if errorlevel 1 (
  echo Error: failed to update user PATH for "%PATH_ENTRY%".
  exit /b 1
)
exit /b 0

:CreateShortcut
set "SHORTCUT_PATH=%~1"
set "ICON_PATH=%DEST_EXE%"
if exist "%DEST_ICON%" set "ICON_PATH=%DEST_ICON%"

powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$shortcutPath = $env:SHORTCUT_PATH;" ^
  "$target = [System.IO.Path]::GetFullPath($env:DEST_EXE);" ^
  "$working = [System.IO.Path]::GetFullPath($env:INSTALL_DIR);" ^
  "$icon = [System.IO.Path]::GetFullPath($env:ICON_PATH);" ^
  "$shell = New-Object -ComObject WScript.Shell;" ^
  "$shortcut = $shell.CreateShortcut($shortcutPath);" ^
  "$shortcut.TargetPath = $target;" ^
  "$shortcut.WorkingDirectory = $working;" ^
  "$shortcut.IconLocation = ($icon + ',0');" ^
  "$shortcut.Save();"
if errorlevel 1 (
  echo Error: failed to create shortcut "%SHORTCUT_PATH%".
  exit /b 1
)
exit /b 0
