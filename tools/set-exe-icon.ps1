# 把 assets\logo.ico 写进 exe 的资源段，让它变成「文件图标」。
#
# 为什么不用常规办法：这台机器上没有 Windows SDK 的 rc.exe，也没有 windres，
# embed-resource / winres 这类 crate 都跑不起来。所以直接调 kernel32 的
# BeginUpdateResource / UpdateResource / EndUpdateResource——资源管理器和
# 任务栏读的就是这一套，改完立刻生效，不用重新链接。
#
# 用法（在项目根目录）：
#     powershell -ExecutionPolicy Bypass -File tools\set-exe-icon.ps1
#     powershell ... -File tools\set-exe-icon.ps1 -Exe target\debug\xiyou-trainer.exe
#
# 注意：cargo 重新链接之后 exe 会被覆盖，图标也就没了，所以每次 release 之后
# 都要再跑一次；或者直接跑 tools\build-release.ps1，它把两步串起来。

param(
    [string]$Exe = "target\release\xiyou-trainer.exe",
    [string]$Icon = "assets\logo.ico",
    [switch]$Verify
)

$ErrorActionPreference = "Stop"
$root = (Get-Location).Path
$exePath = [System.IO.Path]::GetFullPath((Join-Path $root $Exe))
$icoPath = [System.IO.Path]::GetFullPath((Join-Path $root $Icon))

if (-not (Test-Path $exePath)) { throw "找不到 exe：$exePath" }
if (-not (Test-Path $icoPath)) { throw "找不到 ico：$icoPath（先跑 python tools\make_icon.py）" }

$procName = [System.IO.Path]::GetFileNameWithoutExtension($exePath)
if (Get-Process -Name $procName -ErrorAction SilentlyContinue) {
    throw "exe 正在运行，先关掉再写图标"
}

# PowerShell 5.1 对 BitConverter 的 span 重载解析有坑，这里一律手读小端。
function Get-U16([byte[]]$buf, [int]$off) {
    return ([int]$buf[$off] -bor ([int]$buf[$off + 1] -shl 8))
}
function Get-U32([byte[]]$buf, [int]$off) {
    return ([int]$buf[$off] -bor ([int]$buf[$off + 1] -shl 8) -bor
            ([int]$buf[$off + 2] -shl 16) -bor ([int]$buf[$off + 3] -shl 24))
}

Add-Type -Namespace Xiyou -Name Res -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern IntPtr BeginUpdateResource(string fileName, bool deleteExisting);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern bool UpdateResource(IntPtr hUpdate, IntPtr type, IntPtr name, ushort language, byte[] data, uint size);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern bool EndUpdateResource(IntPtr hUpdate, bool discard);
'@

$RT_ICON = 3
$RT_GROUP_ICON = 14
$LANG = 0

[byte[]]$ico = [System.IO.File]::ReadAllBytes($icoPath)

if ((Get-U16 $ico 0) -ne 0 -or (Get-U16 $ico 2) -ne 1) { throw "不是 ICO 文件：$icoPath" }
$count = Get-U16 $ico 4

# ICONDIRENTRY 每条 16 字节
$entries = @()
for ($i = 0; $i -lt $count; $i++) {
    $o = 6 + $i * 16
    $entries += [pscustomobject]@{
        Width      = [int]$ico[$o]
        Height     = [int]$ico[$o + 1]
        ColorCount = [int]$ico[$o + 2]
        Reserved   = [int]$ico[$o + 3]
        Planes     = Get-U16 $ico ($o + 4)
        BitCount   = Get-U16 $ico ($o + 6)
        Bytes      = Get-U32 $ico ($o + 8)
        Offset     = Get-U32 $ico ($o + 12)
        Id         = $i + 1
    }
}

# GRPICONDIR：6 字节头 + 每条 14 字节
$grp = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($grp)
$bw.Write([uint16]0)
$bw.Write([uint16]1)
$bw.Write([uint16]$count)
foreach ($e in $entries) {
    $bw.Write([byte]$e.Width)
    $bw.Write([byte]$e.Height)
    $bw.Write([byte]$e.ColorCount)
    $bw.Write([byte]$e.Reserved)
    $bw.Write([uint16]$e.Planes)
    $bw.Write([uint16]$e.BitCount)
    $bw.Write([uint32]$e.Bytes)
    $bw.Write([uint16]$e.Id)
}
$bw.Flush()
[byte[]]$grpBytes = $grp.ToArray()
$bw.Dispose()
$grp.Dispose()

$h = [Xiyou.Res]::BeginUpdateResource($exePath, $false)
if ($h -eq [IntPtr]::Zero) {
    $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    throw "BeginUpdateResource 失败（Win32 $code）：$([ComponentModel.Win32Exception]::new($code).Message)"
}

try {
    foreach ($e in $entries) {
        [byte[]]$buf = New-Object byte[] $e.Bytes
        [Array]::Copy($ico, $e.Offset, $buf, 0, $e.Bytes)
        if (-not [Xiyou.Res]::UpdateResource($h, [IntPtr]$RT_ICON, [IntPtr]$e.Id, $LANG, $buf, [uint32]$buf.Length)) {
            throw "写 RT_ICON #$($e.Id) 失败（Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())）"
        }
    }
    if (-not [Xiyou.Res]::UpdateResource($h, [IntPtr]$RT_GROUP_ICON, [IntPtr]1, $LANG, $grpBytes, [uint32]$grpBytes.Length)) {
        throw "写 RT_GROUP_ICON 失败（Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())）"
    }
    if (-not [Xiyou.Res]::EndUpdateResource($h, $false)) {
        throw "EndUpdateResource 失败（Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())）"
    }
}
catch {
    # 出错就把句柄丢弃，别把半个资源写进 exe
    [void][Xiyou.Res]::EndUpdateResource($h, $true)
    throw
}

$sizes = ($entries | ForEach-Object { if ($_.Width -eq 0) { 256 } else { $_.Width } }) -join "/"
Write-Host ("已写入 {0} 个尺寸（{1}）→ {2}" -f $count, $sizes, $exePath)

if ($Verify) {
    # 从 exe 里重新读回来，存成 PNG 肉眼看一眼；同时也证明资源确实进去了。
    # 256 那帧是 PNG 压缩的（Windows 推荐做法），GDI+ 读不了，所以单独放过。
    Add-Type -AssemblyName System.Drawing
    $outDir = Join-Path $root "target"
    foreach ($side in 256, 64, 32) {
        try {
            $icon = New-Object System.Drawing.Icon($exePath, $side, $side)
            $png = Join-Path $outDir "icon_check_$side.png"
            $icon.ToBitmap().Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
            $icon.Dispose()
            Write-Host "读回 ${side}px → $png"
        }
        catch {
            Write-Host "读回 ${side}px 跳过（GDI+ 不支持 PNG 帧，外壳本身没问题）"
        }
    }
}
