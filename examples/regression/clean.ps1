# 删除 msceshi 下所有可再生的派生内容与编译产物。
# 用法：pwsh -File clean.ps1
$root = $PSScriptRoot

$files = Get-ChildItem -Recurse -Force -File $root | Where-Object { $_.Extension -in '.exe', '.pdb' }
$sizeBefore = ($files | Measure-Object Length -Sum).Sum
$nFiles = $files.Count
$files | ForEach-Object {
    try { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Stop }
    catch { Write-Warning "删不掉: $($_.FullName.Substring($root.Length + 1)) — $($_.Exception.Message)" }
}

$dirs = Get-ChildItem -Recurse -Force -Directory $root |
    Where-Object { $_.Name -in @('build', 'target', 'mod_module') } |
    Sort-Object { $_.FullName.Length } -Descending
$nDirs = $dirs.Count
$dirs | ForEach-Object {
    try { Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction Stop }
    catch { Write-Warning "删不掉目录: $($_.FullName.Substring($root.Length + 1)) — $($_.Exception.Message)" }
}

if ($sizeBefore -eq $null) { $sizeBefore = 0 }
Write-Host "已删除 $nFiles 个产物文件（$([math]::Round($sizeBefore / 1MB, 2)) MB）与 $nDirs 个派生目录。"
$left = Get-ChildItem -Recurse -Force -File $root
Write-Host "当前 msceshi：$($left.Count) 个文件，$([math]::Round(($left | Measure-Object Length -Sum).Sum / 1KB, 1)) KB。"
