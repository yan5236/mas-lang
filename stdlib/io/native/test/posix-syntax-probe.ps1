# POSIX 源码的语法探测（尽力而为，不参与主测试退出码）。
#
# 本机没有可用的 POSIX 工具链：WSL 被拒绝（E_ACCESSDENIED），MinGW 缺少 lstat()/link()
# 且 mkdir() 只有一参数版本。为了至少覆盖语法与类型层面的错误，这里把 POSIX 源文件
# 复制到临时目录，仅把两参数 mkdir 调用替换成 MinGW 的 _mkdir，并用 -include 补齐
# lstat()/link()/S_ISLNK 声明，然后对每个文件跑两次 gcc -fsyntax-only：
# 一次走默认的 link/unlink 回退分支，一次用 -D__linux__ 走 renameat2 分支。
#
# 因此：通过只说明语法与局部类型自洽，不证明 POSIX 上的实际行为；
# 真正的 POSIX 验证必须由 io-artifacts 工作流在 Linux/macOS runner 上完成。

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$IncludeDir,
    [Parameter(Mandatory = $true)][string[]]$PosixSources
)

$ErrorActionPreference = 'Stop'

function Write-ProbeResult {
    param([string]$Label, [int]$Code)

    if ($Code -eq 0) {
        Write-Host "  ${Label}: 通过"
    } else {
        Write-Host "  ${Label}: 失败（gcc 退出码 $Code）" -ForegroundColor Yellow
    }
}

$gcc = Get-Command gcc -ErrorAction SilentlyContinue
if (-not $gcc) {
    Write-Host 'POSIX 语法探测：跳过（未找到 gcc）。POSIX 运行时行为未验证。'
    exit 0
}

$probeDir = Join-Path $env:TEMP 'mas-io-posix-syntax-probe'
if (Test-Path $probeDir) {
    Remove-Item -Recurse -Force $probeDir
}
New-Item -ItemType Directory -Path $probeDir | Out-Null

$shimPath = Join-Path $probeDir 'posix_probe_shim.h'
$shimLines = @(
    '#ifndef MAS_IO_POSIX_PROBE_SHIM_H',
    '#define MAS_IO_POSIX_PROBE_SHIM_H',
    '/* 仅为 Windows 上的语法探测补齐 MinGW 缺失的 POSIX 声明。 */',
    '#include <sys/types.h>',
    '#include <sys/stat.h>',
    '#include <unistd.h>',
    '#include <direct.h>',
    'extern int lstat(const char* path, struct stat* info);',
    'extern int link(const char* from, const char* to);',
    '#ifndef S_ISLNK',
    '/* MinGW 缺少 POSIX 的 S_ISLNK，这里按公开定义补齐，仅用于语法探测。 */',
    '#define S_ISLNK(mode) (((mode) & S_IFMT) == 0xA000)',
    '#endif',
    '#endif'
)
Set-Content -Path $shimPath -Value $shimLines -Encoding ASCII

Write-Host "POSIX 语法探测：gcc = $($gcc.Source)"
Write-Host '  注意：仅语法检查；mkdir 调用替换为 _mkdir，POSIX 运行时行为未验证。'

$index = 0
foreach ($source in $PosixSources) {
    if (-not (Test-Path $source)) {
        Write-Host "  跳过（缺少 $source）"
        continue
    }

    $sourceText = Get-Content -Path $source -Raw
    $probeText = $sourceText -replace 'return mkdir\(path, \(mode_t\)0777\);', 'return _mkdir(path);'
    $probeSource = Join-Path $probeDir ("io_posix_probe_{0}.c" -f $index)
    $index += 1
    Set-Content -Path $probeSource -Value $probeText -Encoding UTF8

    $name = Split-Path -Leaf $source
    & $gcc.Source -fsyntax-only -std=gnu11 -Wall -Wextra `
        -I $IncludeDir -include $shimPath $probeSource
    Write-ProbeResult -Label "$name（link/unlink 回退分支）" -Code $LASTEXITCODE

    & $gcc.Source -fsyntax-only -std=gnu11 -Wall -Wextra -D__linux__ `
        -I $IncludeDir -include $shimPath $probeSource
    Write-ProbeResult -Label "$name（Linux renameat2 分支）" -Code $LASTEXITCODE
}

Remove-Item -Recurse -Force $probeDir -ErrorAction SilentlyContinue
exit 0
