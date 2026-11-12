# Downloads the pinned Windows libmpv build and makes it usable by the MSVC
# toolchain: an import library (mpv.lib) for linking, and libmpv-2.dll for
# running the program and its tests.
#
# Used by CI and the release workflow. It also works on a developer machine
# with Visual Studio Build Tools and 7-Zip installed:
#
#   pwsh .github/scripts/fetch-libmpv.ps1 -Dest C:\libmpv
#
# In GitHub Actions it also puts the folder on PATH (so the DLL is found at run
# time) and sets RUSTFLAGS so the linker finds mpv.lib.
#
# To move to a newer mpv, pick a weekly `mpv-dev-x86_64-*.7z` from
# https://sourceforge.net/projects/mpv-player-windows/files/libmpv/ (in November
# 2026 SourceForge still listed weekly builds from a year back, while the GitHub
# copies are deleted after about a month), then update $Archive, $Sha256 and
# $MpvCommit together. If the pinned file ever disappears, the download or the
# checksum check fails loudly; pick a newer build then.

param(
    [Parameter(Mandatory = $true)][string]$Dest
)

$ErrorActionPreference = 'Stop'

$Archive   = 'mpv-dev-x86_64-20261108-git-55d3feb45c.7z'
$Sha256    = '7c147cb7ceaba717414e782ef4398de728f679db6e86daaa8aeb175670cb7994'
$MpvCommit = '55d3feb45c'
$Url       = "https://sourceforge.net/projects/mpv-player-windows/files/libmpv/$Archive/download"

New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$Dest = (Resolve-Path $Dest).Path
$archivePath = Join-Path $Dest $Archive

# 1. Download (SourceForge redirects to a mirror; retry if a mirror is flaky).
if (-not (Test-Path $archivePath)) {
    & curl.exe --fail --location --silent --show-error --retry 5 --retry-delay 5 --output $archivePath $Url
    if ($LASTEXITCODE -ne 0) { throw "Downloading $Url failed" }
}

# 2. Verify it is exactly the archive we pinned.
$actual = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLower()
if ($actual -ne $Sha256) {
    Remove-Item $archivePath
    throw "Checksum mismatch for ${Archive}: expected $Sha256, got $actual"
}

# 3. Extract libmpv-2.dll, libmpv.dll.a and include/.
$sevenZip = (Get-Command 7z -ErrorAction SilentlyContinue).Source
if (-not $sevenZip) { $sevenZip = 'C:\Program Files\7-Zip\7z.exe' }
& $sevenZip x -y "-o$Dest" $archivePath | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Extracting $Archive failed" }

# 4. Build an MSVC import library. The archive no longer ships mpv.def, so the
#    exports are read from the DLL itself.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$dumpbin = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
if (-not $dumpbin) { throw 'dumpbin.exe not found: install the MSVC build tools' }
$lib = Join-Path (Split-Path $dumpbin) 'lib.exe'

$exports = & $dumpbin /nologo /exports (Join-Path $Dest 'libmpv-2.dll') |
    ForEach-Object { if ($_ -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)') { $Matches[1] } }
if (-not $exports) { throw 'No exports found in libmpv-2.dll' }
$def = Join-Path $Dest 'mpv.def'
@('LIBRARY libmpv-2.dll', 'EXPORTS') + $exports | Set-Content -Encoding ascii $def
& $lib /nologo "/def:$def" /machine:x64 "/out:$(Join-Path $Dest 'mpv.lib')" | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'lib.exe failed to build mpv.lib' }

# 5. mpv's licence texts, shipped next to the DLL in release zips.
foreach ($name in 'Copyright', 'LICENSE.GPL', 'LICENSE.LGPL') {
    & curl.exe --fail --location --silent --show-error --retry 5 `
        --output (Join-Path $Dest "mpv-$name.txt") `
        "https://raw.githubusercontent.com/mpv-player/mpv/$MpvCommit/$name"
    if ($LASTEXITCODE -ne 0) { throw "Downloading mpv's $name failed" }
}

Write-Host "libmpv ready in $Dest ($($exports.Count) exports)"

if ($env:GITHUB_PATH) { Add-Content $env:GITHUB_PATH $Dest }
if ($env:GITHUB_ENV)  { Add-Content $env:GITHUB_ENV "RUSTFLAGS=-L native=$Dest" }
