$ErrorActionPreference = 'Continue';
$removed = 0;
$removedFiles = 0;
[long]$freedBytes = 0;
$failed = 0;
function Get-QboaTreeStats([string]$target) {
    $files = @()
    if (Test-Path -LiteralPath $target -PathType Leaf) {
        $files = @(Get-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue)
    } elseif (Test-Path -LiteralPath $target -PathType Container) {
        $files = @(Get-ChildItem -LiteralPath $target -Recurse -Force -File -ErrorAction SilentlyContinue)
    }
    $size = ($files | Measure-Object -Property Length -Sum).Sum
    return @{ Files = $files.Count; Bytes = [long]$(if ($size) { $size } else { 0 }) }
}
foreach ($path in @($env:TEMP, "$env:WINDIR\Temp")) {
    if (Test-Path -LiteralPath $path) {
        Write-Output ("Limpando temporários: " + $path);
        foreach ($item in @(Get-ChildItem -LiteralPath $path -Force -ErrorAction SilentlyContinue)) {
            try {
                $stats = Get-QboaTreeStats $item.FullName
                $removedFiles += $stats.Files
                $freedBytes += $stats.Bytes
                Remove-Item -LiteralPath $item.FullName -Recurse -Force -ErrorAction Stop;
                $removed++;
            } catch {
                $failed++;
                Write-Output ("Não foi possível remover: " + $item.FullName + " — " + $_.Exception.Message);
            }
        }
    }
}
$browserRoots = @(
    "$env:LOCALAPPDATA\Google\Chrome\User Data",
    "$env:LOCALAPPDATA\Microsoft\Edge\User Data",
    "$env:LOCALAPPDATA\BraveSoftware\Brave-Browser\User Data",
    "$env:LOCALAPPDATA\Vivaldi\User Data",
    "$env:APPDATA\Opera Software\Opera Stable",
    "$env:APPDATA\Mozilla\Firefox\Profiles"
)
$cacheNames = @('Cache', 'Cache2', 'Code Cache', 'GPUCache', 'StartupCache', 'shader-cache', 'CacheStorage')
if ($env:QBOA_CLEAN_BROWSER_CACHE -eq '1') {
foreach ($root in $browserRoots) {
    if (Test-Path -LiteralPath $root) {
        Write-Output ("Procurando caches do navegador: " + $root);
        $cacheDirs = @(Get-ChildItem -LiteralPath $root -Directory -Recurse -Force -ErrorAction SilentlyContinue |
            Where-Object { $cacheNames -contains $_.Name -or $_.FullName -match '\\Service Worker\\CacheStorage$' } |
            Sort-Object { $_.FullName.Length } -Descending);
        foreach ($dir in $cacheDirs) {
            if (Test-Path -LiteralPath $dir.FullName) {
                try {
                    $stats = Get-QboaTreeStats $dir.FullName
                    $removedFiles += $stats.Files
                    $freedBytes += $stats.Bytes
                    Remove-Item -LiteralPath $dir.FullName -Recurse -Force -ErrorAction Stop;
                    $removed++;
                } catch {
                    $failed++;
                    Write-Output ("Não foi possível remover cache: " + $dir.FullName + " — " + $_.Exception.Message);
                }
            }
        }
    }
}
} else {
    Write-Output 'Caches dos navegadores ignorados conforme sua escolha.';
}
try {
    Clear-RecycleBin -Force -ErrorAction Stop;
    Write-Output 'Lixeira esvaziada.';
} catch {
    Write-Output ("Lixeira não esvaziada: " + $_.Exception.Message);
}
Write-Output ("Limpeza padrão: {0} item(ns)/pasta(s) removido(s), {1} falha(s)." -f $removed, $failed);
Write-Output ("QBOA_METRICS files={0} bytes={1} registry=0" -f $removedFiles, $freedBytes);

