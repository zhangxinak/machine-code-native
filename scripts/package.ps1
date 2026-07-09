param(
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
$OutDir = Join-Path $Dist $PackageName
$ExePath = Join-Path $Root "target\$Target\release\machine-code-native.exe"

Push-Location $Root
try {
    cargo build --release --target $Target

    if (!(Test-Path $ExePath)) {
        throw "未找到构建产物: $ExePath"
    }

    if (Test-Path $OutDir) {
        Remove-Item -LiteralPath $OutDir -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

    Copy-Item -LiteralPath $ExePath -Destination (Join-Path $OutDir "机器码获取工具-Native.exe")

    @'
@echo off
chcp 65001 >nul
echo ========================================
echo 机器码获取工具 Native 版 - 诊断脚本
echo ========================================
echo.
echo 程序目录: %~dp0
echo 日志路径: %APPDATA%\machine-code-native\startup.log
echo.
echo [1] 启动程序
start "" "%~dp0机器码获取工具-Native.exe"
timeout /t 3 >nul
echo.
echo [2] 检查本地 API
powershell -NoProfile -ExecutionPolicy Bypass -Command "try { Invoke-WebRequest -UseBasicParsing http://127.0.0.1:18888/health | Select-Object -ExpandProperty Content } catch { $_.Exception.Message }"
echo.
echo [3] 输出日志
if exist "%APPDATA%\machine-code-native\startup.log" (
  type "%APPDATA%\machine-code-native\startup.log"
) else (
  echo 未生成日志。
)
echo.
pause
'@ | Set-Content -Path (Join-Path $OutDir "诊断.bat") -Encoding OEM

    @"
机器码获取工具 Native 版

使用方式：
1. 双击「机器码获取工具-Native.exe」。
2. 点击「开启授权」后，工具会采集机器码。
3. 网页可访问：http://127.0.0.1:18888/api/machine-code
4. 若异常，双击「诊断.bat」，或查看：
   %APPDATA%\machine-code-native\startup.log

说明：
- 本版不依赖 WebView2、Edge、Tauri、Electron。
- 本版为 portable 版本，解压即可运行，不需要安装器。
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
