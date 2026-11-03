$ErrorActionPreference = 'Continue'
$removedFiles = 0
[long]$freedBytes = 0
$failed = 0
$processedProfiles = 0

function Add-QboaRemovedFile([string]$Path) {
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        try {
            $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
            $size = [long]$item.Length
            Remove-Item -LiteralPath $Path -Force -ErrorAction Stop
            $script:removedFiles++
            $script:freedBytes += $size
        } catch {
            $script:failed++
            Write-Output ("Não foi possível remover {0}: {1}" -f $Path, $_.Exception.Message)
        }
    }
}

if ($env:QBOA_CLEAN_BROWSER_CACHE -ne '1') {
    Write-Output 'Histórico dos navegadores preservado conforme sua escolha de manter os navegadores abertos.'
} else {
    $chromiumRoots = @(
        "$env:LOCALAPPDATA\Google\Chrome\User Data",
        "$env:LOCALAPPDATA\Microsoft\Edge\User Data",
        "$env:LOCALAPPDATA\BraveSoftware\Brave-Browser\User Data",
        "$env:LOCALAPPDATA\Vivaldi\User Data",
        "$env:APPDATA\Opera Software\Opera Stable",
        "$env:APPDATA\Opera Software\Opera GX Stable"
    ) | Where-Object { Test-Path -LiteralPath $_ }

    Write-Output '[Etapa 1/1] Localizando perfis e limpando históricos...'

    foreach ($root in $chromiumRoots) {
        $historyFiles = @(Get-ChildItem -LiteralPath $root -Filter 'History' -File -Recurse -Force -ErrorAction SilentlyContinue)
        foreach ($file in $historyFiles) {
            Write-Output ("Limpando histórico Chromium: " + $file.DirectoryName)
            Add-QboaRemovedFile $file.FullName
            foreach ($suffix in @('-journal', '-wal', '-shm')) {
                Add-QboaRemovedFile ($file.FullName + $suffix)
            }
            $processedProfiles++
        }
    }

    $firefoxRoots = @(
        "$env:APPDATA\Mozilla\Firefox\Profiles",
        "$env:LOCALAPPDATA\Mozilla\Firefox\Profiles"
    ) | Where-Object { Test-Path -LiteralPath $_ }

    if (-not ('QboaNativeSqlite' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class QboaNativeSqlite {
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Unicode)]
    private static extern int sqlite3_open16(string filename, out IntPtr db);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Ansi)]
    private static extern int sqlite3_exec(IntPtr db, string sql, IntPtr callback, IntPtr arg, out IntPtr error);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr sqlite3_errmsg(IntPtr db);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern int sqlite3_close(IntPtr db);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    private static extern void sqlite3_free(IntPtr value);
    public static string Execute(string filename, string sql) {
        IntPtr db;
        int result = sqlite3_open16(filename, out db);
        if (result != 0) throw new Exception("SQLite open: " + result);
        try {
            IntPtr error;
            result = sqlite3_exec(db, sql, IntPtr.Zero, IntPtr.Zero, out error);
            if (result != 0) {
                string message = error != IntPtr.Zero ? Marshal.PtrToStringAnsi(error) : Marshal.PtrToStringAnsi(sqlite3_errmsg(db));
                if (error != IntPtr.Zero) sqlite3_free(error);
                throw new Exception("SQLite: " + message);
            }
            return "ok";
        } finally {
            sqlite3_close(db);
        }
    }
}
'@
    }

    foreach ($root in $firefoxRoots) {
        $placesFiles = @(Get-ChildItem -LiteralPath $root -Filter 'places.sqlite' -File -Recurse -Force -ErrorAction SilentlyContinue)
        foreach ($database in $placesFiles) {
            try {
                $before = [long](Get-Item -LiteralPath $database.FullName -Force).Length
                $sql = 'PRAGMA foreign_keys=ON; BEGIN IMMEDIATE; DELETE FROM moz_historyvisits; DELETE FROM moz_inputhistory; DELETE FROM moz_places WHERE id NOT IN (SELECT fk FROM moz_bookmarks WHERE fk IS NOT NULL); COMMIT; VACUUM;'
                [QboaNativeSqlite]::Execute($database.FullName, $sql) | Out-Null
                $after = [long](Get-Item -LiteralPath $database.FullName -Force).Length
                $freedBytes += [Math]::Max(0, $before - $after)
                foreach ($suffix in @('-journal', '-wal', '-shm')) {
                    Add-QboaRemovedFile ($database.FullName + $suffix)
                }
                $processedProfiles++
                Write-Output ("Histórico Firefox limpo; favoritos preservados: " + $database.DirectoryName)
            } catch {
                $failed++
                Write-Output ("Não foi possível limpar o histórico Firefox em {0}: {1}" -f $database.DirectoryName, $_.Exception.Message)
            }
        }
    }

    if ($processedProfiles -eq 0) {
        Write-Output 'Nenhum perfil compatível de navegador foi encontrado.'
    }
}
Write-Output ("Histórico dos navegadores: {0} perfil(is) processado(s), {1} arquivo(s) removido(s), {2} falha(s)." -f $processedProfiles, $removedFiles, $failed)
Write-Output ("QBOA_METRICS files={0} bytes={1} registry=0" -f $removedFiles, $freedBytes)
