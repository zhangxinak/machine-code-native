param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [Parameter(Mandatory = $true)]
    [ValidateSet("x86", "x64")]
    [string]$Architecture
)

$ErrorActionPreference = "Stop"

$ExpectedMachine = if ($Architecture -eq "x86") { 0x014c } else { 0x8664 }
$ExpectedOptionalMagic = if ($Architecture -eq "x86") { 0x010b } else { 0x020b }
$ResolvedPath = (Resolve-Path -LiteralPath $Path).Path
$Bytes = [IO.File]::ReadAllBytes($ResolvedPath)

if ($Bytes.Length -lt 256) {
    throw "PE 文件过短: $ResolvedPath ($($Bytes.Length) bytes)"
}

if ($Bytes[0] -ne 0x4d -or $Bytes[1] -ne 0x5a) {
    throw "DOS 签名无效，不是 Windows PE 文件: $ResolvedPath"
}

$PeOffset = [BitConverter]::ToInt32($Bytes, 0x3c)
if ($PeOffset -lt 0 -or $PeOffset + 88 -gt $Bytes.Length) {
    throw "PE 头偏移无效: $PeOffset"
}

if ($Bytes[$PeOffset] -ne 0x50 -or $Bytes[$PeOffset + 1] -ne 0x45 -or
    $Bytes[$PeOffset + 2] -ne 0 -or $Bytes[$PeOffset + 3] -ne 0) {
    throw "PE 签名无效: $ResolvedPath"
}

$Machine = [BitConverter]::ToUInt16($Bytes, $PeOffset + 4)
$OptionalHeader = $PeOffset + 24
$OptionalMagic = [BitConverter]::ToUInt16($Bytes, $OptionalHeader)
$MajorOsVersion = [BitConverter]::ToUInt16($Bytes, $OptionalHeader + 40)
$MinorOsVersion = [BitConverter]::ToUInt16($Bytes, $OptionalHeader + 42)
$Subsystem = [BitConverter]::ToUInt16($Bytes, $OptionalHeader + 68)

if ($Machine -ne $ExpectedMachine) {
    throw ("架构不匹配: expected={0} (0x{1:X4}), actual=0x{2:X4}, file={3}" -f $Architecture, $ExpectedMachine, $Machine, $ResolvedPath)
}

if ($OptionalMagic -ne $ExpectedOptionalMagic) {
    throw ("PE optional header 不匹配: expected=0x{0:X4}, actual=0x{1:X4}" -f $ExpectedOptionalMagic, $OptionalMagic)
}

if ($Subsystem -ne 2) {
    throw "不是 Windows GUI 子系统: subsystem=$Subsystem"
}

$Hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ResolvedPath).Hash.ToLowerInvariant()
Write-Host ("PE 验证通过: arch={0}, machine=0x{1:X4}, os_version={2}.{3}, subsystem={4}, bytes={5}, sha256={6}" -f `
    $Architecture, $Machine, $MajorOsVersion, $MinorOsVersion, $Subsystem, $Bytes.Length, $Hash)
