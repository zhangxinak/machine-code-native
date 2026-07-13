param(
    [ValidateSet("x86_64-pc-windows-msvc", "i686-pc-windows-msvc")]
    [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$Dist = Join-Path $Root "dist"
$PackageName = if ($Target -eq "i686-pc-windows-msvc") {
    "machine-code-native-windows-x86"
} else {
    "machine-code-native-windows-x64"
}
$Architecture = if ($Target -eq "i686-pc-windows-msvc") { "x86" } else { "x64" }
$OutDir = Join-Path $Dist $PackageName
$ExePath = Join-Path $Root "target\$Target\release\machine-code-native.exe"
$VerifyPeScript = Join-Path $PSScriptRoot "verify-pe.ps1"

Push-Location $Root
try {
    cargo build --release --target $Target

    if (!(Test-Path $ExePath)) {
        throw "未找到构建产物: $ExePath"
    }

    & $VerifyPeScript -Path $ExePath -Architecture $Architecture

    if (Test-Path $OutDir) {
        Remove-Item -LiteralPath $OutDir -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    $PackagedExePath = Join-Path $OutDir "machine-code-native.exe"
    Copy-Item -LiteralPath $ExePath -Destination $PackagedExePath
    & $VerifyPeScript -Path $PackagedExePath -Architecture $Architecture
    $ExeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $PackagedExePath).Hash.ToLowerInvariant()
    "${ExeHash}  machine-code-native.exe" | Set-Content -Path (Join-Path $OutDir "SHA256SUMS.txt") -Encoding ASCII

    @'
@echo off
chcp 65001 >nul
echo ========================================
echo Machine Code Native - diagnostics
echo ========================================
echo.
echo Program dir: %~dp0
echo Log path: %APPDATA%\machine-code-native\startup.log
echo.
echo [1] Environment and executable
ver
echo Process architecture: %PROCESSOR_ARCHITECTURE%
echo WOW64 architecture: %PROCESSOR_ARCHITEW6432%
set "MACHINE_CODE_EXE=%~dp0machine-code-native.exe"
powershell -NoProfile -ExecutionPolicy Bypass -Command "try { $b = [IO.File]::ReadAllBytes($env:MACHINE_CODE_EXE); $p = [BitConverter]::ToInt32($b, 60); $m = [BitConverter]::ToUInt16($b, $p + 4); $h = [Security.Cryptography.SHA256]::Create(); try { $hash = [BitConverter]::ToString($h.ComputeHash($b)).Replace('-', '').ToLowerInvariant() } finally { $h.Dispose() }; 'PE machine: 0x{0:X4} (x86=0x014C, x64=0x8664)' -f $m; 'SHA256: ' + $hash } catch { 'Executable inspection failed: ' + $_.Exception.Message }"
echo.
echo [2] Start program
start "" "%~dp0machine-code-native.exe"
timeout /t 3 >nul
echo.
echo [3] Check localhost API
powershell -NoProfile -ExecutionPolicy Bypass -Command "try { [Console]::OutputEncoding = [Text.Encoding]::UTF8; $request = [Net.HttpWebRequest]::Create('http://127.0.0.1:18888/health'); $request.Method = 'GET'; $request.Timeout = 3000; $request.ReadWriteTimeout = 3000; $response = $request.GetResponse(); try { $reader = New-Object IO.StreamReader($response.GetResponseStream(), [Text.Encoding]::UTF8); $reader.ReadToEnd() } finally { if ($null -ne $reader) { $reader.Dispose() }; if ($null -ne $response) { $response.Close() } } } catch { $_.Exception.Message }"
echo.
echo [4] Print log
if exist "%APPDATA%\machine-code-native\startup.log" (
  type "%APPDATA%\machine-code-native\startup.log"
) else (
  echo Log file was not created.
)
echo.
pause
'@ | Set-Content -Path (Join-Path $OutDir "诊断.bat") -Encoding ASCII

    @"
机器码获取工具 Native 版

使用方式：
1. 双击「machine-code-native.exe」。
2. 点击「开启授权」后，工具会采集机器码。
3. 网页可访问：http://127.0.0.1:18888/api/machine-code
4. 若异常，双击「诊断.bat」。日志路径：
   %APPDATA%\machine-code-native\startup.log

说明：
- 本版不依赖 WebView2、Edge、Tauri、Electron。
- 本版为 portable 版本，解压即可运行，不需要安装器。
- x86 包用于 32 位 Windows，x64 包用于 64 位 Windows；两者均要求 Windows 7 或更高版本。
- 如果主板/CPU/硬盘序列号取不到，界面和日志会显示具体失败原因。
"@ | Set-Content -Path (Join-Path $OutDir "使用说明.txt") -Encoding UTF8

    $Zip = Join-Path $Dist "$PackageName.zip"
    if (Test-Path $Zip) {
        Remove-Item -LiteralPath $Zip -Force
    }
    Compress-Archive -Path (Join-Path $OutDir "*") -DestinationPath $Zip

    Write-Host "打包完成: $Zip"
}
finally {
    Pop-Location
}
