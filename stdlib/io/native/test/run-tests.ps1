# 独立 C 测试驱动：用本机 cl.exe 编译 stdlib/io 的原生源码与测试桩，跑真实文件断言。
# 不经过 Mas 工具链，也不需要 cargo；只依赖 Visual Studio 生成工具。
#
# 用法：
#   pwsh -File native/test/run-tests.ps1
#   pwsh -File native/test/run-tests.ps1 -SkipPosixProbe
#
# 退出码：0 全部断言通过；1 存在失败断言；90 找不到或调用 vcvars64.bat 失败；91 编译失败。

[CmdletBinding()]
param(
    [string]$VcVars,
    [switch]$SkipPosixProbe,
    [switch]$KeepBuild
)

$ErrorActionPreference = 'Stop'

$testRoot = $PSScriptRoot
$nativeRoot = Split-Path -Parent $testRoot
$packageRoot = Split-Path -Parent $nativeRoot
$includeDir = Join-Path $nativeRoot 'include'

$sources = @(
    (Join-Path $nativeRoot 'common\io_common.c'),
    (Join-Path $nativeRoot 'common\io_state.c'),
    (Join-Path $nativeRoot 'common\io_stream.c'),
    (Join-Path $nativeRoot 'common\io_whole.c'),
    (Join-Path $nativeRoot 'common\io_paths.c'),
    (Join-Path $nativeRoot 'platform\windows\io_windows.c'),
    (Join-Path $nativeRoot 'platform\windows\io_windows_fs.c'),
    (Join-Path $testRoot 'io_host_test.c'),
    (Join-Path $testRoot 'io_test_api.c'),
    (Join-Path $testRoot 'io_host_test_stream.c'),
    (Join-Path $testRoot 'io_host_test_text.c'),
    (Join-Path $testRoot 'io_host_test_std.c'),
    (Join-Path $testRoot 'io_host_test_fs.c'),
    (Join-Path $testRoot 'io_host_test_links.c')
)

foreach ($source in $sources) {
    if (-not (Test-Path $source)) {
        Write-Host "缺少源文件：$source" -ForegroundColor Red
        exit 91
    }
}

# 在候选安装根目录、vswhere 和已知 cl.exe 位置里按顺序找 vcvars64.bat。
function Find-VcVars64 {
    param([string]$Explicit)

    $candidates = New-Object System.Collections.Generic.List[string]
    if ($Explicit) { $candidates.Add($Explicit) }
    if ($env:MAS_IO_VCVARS64) { $candidates.Add($env:MAS_IO_VCVARS64) }
    if ($env:VSINSTALLDIR) {
        $candidates.Add((Join-Path $env:VSINSTALLDIR 'VC\Auxiliary\Build\vcvars64.bat'))
    }
    if ($env:VCINSTALLDIR) {
        $candidates.Add((Join-Path $env:VCINSTALLDIR 'Auxiliary\Build\vcvars64.bat'))
        $candidates.Add((Join-Path (Split-Path -Parent $env:VCINSTALLDIR) 'VC\Auxiliary\Build\vcvars64.bat'))
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $vswhere) {
        $found = & $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath 2>$null
        foreach ($installation in @($found)) {
            if ($installation) {
                $candidates.Add((Join-Path $installation 'VC\Auxiliary\Build\vcvars64.bat'))
            }
        }
    }

    $knownRoots = @(
        'E:\c\BuildTools',
        'C:\Program Files\Microsoft Visual Studio\2022\BuildTools',
        'C:\Program Files\Microsoft Visual Studio\2022\Community',
        'C:\Program Files\Microsoft Visual Studio\2022\Professional',
        'C:\Program Files\Microsoft Visual Studio\2022\Enterprise',
        'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools'
    )
    foreach ($root in $knownRoots) {
        $candidates.Add((Join-Path $root 'VC\Auxiliary\Build\vcvars64.bat'))
    }

    # 从任何已安装 cl.exe 反推同一套生成工具里的 vcvars64.bat。
    $patterns = @(
        'E:\c\BuildTools\VC\Tools\MSVC\*\bin\HostX64\x64\cl.exe',
        'C:\Program Files\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*\bin\HostX64\x64\cl.exe',
        'C:\Program Files (x86)\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*\bin\HostX64\x64\cl.exe'
    )
    foreach ($pattern in $patterns) {
        foreach ($compiler in @(Get-ChildItem -Path $pattern -ErrorAction SilentlyContinue)) {
            $directory = $compiler.Directory
            for ($level = 0; $level -lt 6 -and $directory; $level += 1) {
                $candidate = Join-Path $directory.FullName 'VC\Auxiliary\Build\vcvars64.bat'
                if (Test-Path $candidate) {
                    $candidates.Add($candidate)
                    break
                }
                $directory = $directory.Parent
            }
        }
    }

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) {
            return (Resolve-Path $candidate).Path
        }
    }
    return $null
}

$vcvarsPath = Find-VcVars64 -Explicit $VcVars
if (-not $vcvarsPath) {
    Write-Host '未找到 vcvars64.bat：请安装 Visual Studio 生成工具，或用 -VcVars 指定完整路径。' -ForegroundColor Red
    exit 90
}
Write-Host "vcvars64.bat: $vcvarsPath"

$buildDir = Join-Path $env:TEMP 'mas-io-native-tests'
if (Test-Path $buildDir) {
    Remove-Item -Recurse -Force $buildDir
}
New-Item -ItemType Directory -Path $buildDir | Out-Null
$executable = Join-Path $buildDir 'io_host_test.exe'
$batchPath = Join-Path $buildDir 'build-and-run.bat'

$quotedSources = ($sources | ForEach-Object { '"' + $_ + '"' }) -join ' '
$batchLines = @(
    '@echo off',
    ('call "{0}" >nul' -f $vcvarsPath),
    'if errorlevel 1 exit /b 90',
    ('pushd "{0}"' -f $buildDir),
    ('cl /nologo /std:c11 /utf-8 /W4 /WX /I"{0}" /Fe:"{1}" {2}' -f $includeDir, $executable, $quotedSources),
    'set "rc=%errorlevel%"',
    'popd',
    'if not "%rc%"=="0" exit /b 91',
    ('"{0}"' -f $executable),
    'exit /b %errorlevel%'
)
Set-Content -Path $batchPath -Value $batchLines -Encoding ASCII

Write-Host "编译 $($sources.Count) 个源文件 ..."
& cmd.exe /c $batchPath
$exitCode = $LASTEXITCODE

switch ($exitCode) {
    0 { Write-Host '原生 IO 测试：全部断言通过。' -ForegroundColor Green }
    90 { Write-Host '调用 vcvars64.bat 失败。' -ForegroundColor Red }
    91 { Write-Host 'cl.exe 编译失败。' -ForegroundColor Red }
    default { Write-Host "原生 IO 测试：存在失败断言（退出码 $exitCode）。" -ForegroundColor Red }
}

# POSIX 源在本机没有原生工具链：这里只做尽力而为的语法探测，结果不影响退出码。
if (-not $SkipPosixProbe) {
    & (Join-Path $testRoot 'posix-syntax-probe.ps1') -IncludeDir $includeDir -PosixSources @(
        (Join-Path $nativeRoot 'platform\posix\io_posix.c'),
        (Join-Path $nativeRoot 'platform\posix\io_posix_fs.c')
    )
}

if (-not $KeepBuild) {
    Remove-Item -Recurse -Force $buildDir -ErrorAction SilentlyContinue
}
exit $exitCode
