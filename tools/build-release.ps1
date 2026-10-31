# 一步构建 release 并给 exe 打上图标。
#
# cargo 每次重新链接都会覆盖 exe，资源段里画的图标也就跟着没了，
# 所以「链接完再写资源」必须是固定动作，别指望一次手动搞定。
#
# 用法（在项目根目录）：
#     powershell -ExecutionPolicy Bypass -File tools\build-release.ps1

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# cargo 把「Finished ...」写到 stderr，在 $ErrorActionPreference = Stop 下
# 会被 PowerShell 当成异常抛出来。这里把它当普通文本接住，只看退出码。
$prev = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$lines = & cargo build --release 2>&1
$code = $LASTEXITCODE
$ErrorActionPreference = $prev
$lines | ForEach-Object { Write-Host "$_" }
if ($code -ne 0) { throw "cargo build 失败（$code）" }

& (Join-Path $root "tools\set-exe-icon.ps1")

$exe = Join-Path $root "target\release\xiyou-trainer.exe"
$size = [Math]::Round((Get-Item $exe).Length / 1MB, 2)
Write-Host "完成：$exe（$size MB）"
