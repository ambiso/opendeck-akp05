[CmdletBinding()]
param(
    # x64 Microsoft.VC*.CRT directory from the Visual C++ redistributable.
    [Parameter(Mandatory)][string]$CrtDirectory
)
$ErrorActionPreference = 'Stop'
$crt = (Resolve-Path -LiteralPath $CrtDirectory).Path
Push-Location (Split-Path $PSScriptRoot)
try {
    $target = 'x86_64-pc-windows-msvc'
    $build = & cargo build --locked --release --target $target --message-format=json-render-diagnostics
    if ($LASTEXITCODE) { throw "Cargo release build failed ($LASTEXITCODE)" }
    $exe = @($build | ConvertFrom-Json | Where-Object {
        $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'opendeck-akp05' -and $_.executable
    } | Select-Object -ExpandProperty executable)
    if ($exe.Count -ne 1) { throw 'Expected one Windows plugin executable' }

    $manifest = Get-Content manifest.json -Raw | ConvertFrom-Json
    $stage = Join-Path 'target' ('windows-package-' + [guid]::NewGuid())
    $dir = New-Item -ItemType Directory -Path (Join-Path $stage ($manifest.PluginUUID + '.sdPlugin'))
    Copy-Item -LiteralPath $exe[0] -Destination (Join-Path $dir.FullName $manifest.CodePathWin)
    Copy-Item -LiteralPath manifest.json, LICENSE, docs/continuous-strip.md, vendor/mirabox/transport.dll -Destination $dir.FullName
    Copy-Item -LiteralPath assets -Recurse -Destination $dir.FullName
    Copy-Item -LiteralPath vendor/mirabox/LICENSE -Destination (Join-Path $dir.FullName 'LICENSE-Mirabox.txt')
    Copy-Item -LiteralPath vendor/jpeg-notices -Recurse -Destination $dir.FullName
    foreach ($name in 'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
        Copy-Item -LiteralPath (Join-Path $crt $name) -Destination $dir.FullName
    }
    $out = New-Item -ItemType Directory -Path build -Force
    $zip = Join-Path $out.FullName "Interstitial-N4-Pro-$($manifest.Version)-win-x64.zip"
    $tmp = Join-Path $stage 'package.zip'
    Compress-Archive -LiteralPath $dir.FullName -DestinationPath $tmp
    [IO.File]::Move((Resolve-Path $tmp).Path, $zip, $true)
    Get-FileHash -LiteralPath $zip
} finally {
    Pop-Location
}
