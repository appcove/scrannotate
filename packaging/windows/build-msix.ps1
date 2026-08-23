<#
.SYNOPSIS
  Build an MSIX package of scrannotate for the Microsoft Store (or sideload).

.DESCRIPTION
  Stages a package layout (exe + manifest + assets), fills the manifest's
  @PLACEHOLDER@ identity tokens from parameters/environment, and runs
  makeappx. For Store submission you upload the UNSIGNED .msix — the Store
  re-signs it. For local sideload testing, pass -SignCert to Authenticode-sign
  it with a cert whose Subject matches the Publisher (see docs/SIGNING.md).

  Requires the Windows SDK (makeappx.exe, signtool.exe on PATH or under
  "C:\Program Files (x86)\Windows Kits\10\bin\...\x64").

.EXAMPLE
  # Store package (unsigned; the Store signs it):
  ./build-msix.ps1 -Exe target/release/scrannotate.exe -Version 0.4.0.0 -Arch x64

  # Sideload package signed for local testing:
  ./build-msix.ps1 -Exe target/release/scrannotate.exe -Version 0.4.0.0 -Arch x64 `
      -SignCert scrannotate-test.pfx -SignPassword (Read-Host -AsSecureString)
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)] [string] $Exe,
  [Parameter(Mandatory)] [string] $Version,          # four-part, e.g. 0.4.0.0
  [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
  [string] $IdentityName        = $env:MSIX_IDENTITY_NAME,
  [string] $Publisher           = $env:MSIX_PUBLISHER,           # CN=... exactly as in Partner Center
  [string] $PublisherDisplayName = $env:MSIX_PUBLISHER_DISPLAY_NAME,
  [string] $OutDir              = 'dist',
  [string] $SignCert,                                            # .pfx for sideload signing (optional)
  [System.Security.SecureString] $SignPassword
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

foreach ($pair in @{ IdentityName = $IdentityName; Publisher = $Publisher; PublisherDisplayName = $PublisherDisplayName }.GetEnumerator()) {
  if ([string]::IsNullOrWhiteSpace($pair.Value)) {
    throw "Missing -$($pair.Key) (or the matching MSIX_* env var) — see docs/SIGNING.md"
  }
}
if (-not (Test-Path $Exe)) { throw "Executable not found: $Exe" }

function Find-SdkTool([string] $name) {
  $cmd = Get-Command $name -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  $root = 'C:\Program Files (x86)\Windows Kits\10\bin'
  $found = Get-ChildItem -Path $root -Recurse -Filter $name -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $found) { throw "$name not found — install the Windows SDK" }
  return $found.FullName
}

# Stage the layout.
$layout = Join-Path ([System.IO.Path]::GetTempPath()) ("scrannotate-msix-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $layout | Out-Null
try {
  Copy-Item $Exe (Join-Path $layout 'scrannotate.exe')

  $assetsSrc = Join-Path $here 'Assets'
  $assetsDst = Join-Path $layout 'Assets'
  if (Test-Path $assetsSrc) {
    Copy-Item $assetsSrc $assetsDst -Recurse
  } else {
    Write-Warning "No Assets\ folder — the Store requires tile/logo PNGs (see docs/SIGNING.md)"
    New-Item -ItemType Directory -Path $assetsDst | Out-Null
  }

  # Fill the manifest.
  $manifest = Get-Content (Join-Path $here 'AppxManifest.xml') -Raw
  $manifest = $manifest.
    Replace('@IDENTITY_NAME@', $IdentityName).
    Replace('@PUBLISHER@', $Publisher).
    Replace('@PUBLISHER_DISPLAY_NAME@', $PublisherDisplayName).
    Replace('@VERSION@', $Version).
    Replace('@ARCH@', $Arch)
  Set-Content -Path (Join-Path $layout 'AppxManifest.xml') -Value $manifest -Encoding UTF8

  # Pack.
  New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  $msix = Join-Path $OutDir "scrannotate-$Arch.msix"
  $makeappx = Find-SdkTool 'makeappx.exe'
  & $makeappx pack /d $layout /p $msix /o
  if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }

  # Optional sideload signing.
  if ($SignCert) {
    $signtool = Find-SdkTool 'signtool.exe'
    $pw = if ($SignPassword) {
      [System.Runtime.InteropServices.Marshal]::PtrToStringAuto(
        [System.Runtime.InteropServices.Marshal]::SecureStringToBSTR($SignPassword))
    } else { '' }
    & $signtool sign /fd SHA256 /a /f $SignCert /p $pw $msix
    if ($LASTEXITCODE -ne 0) { throw "signtool failed ($LASTEXITCODE)" }
    Write-Host "Signed $msix for sideload testing."
  } else {
    Write-Host "Built (unsigned) $msix — upload to Partner Center; the Store signs it."
  }
  Write-Host "Output: $msix"
} finally {
  Remove-Item $layout -Recurse -Force -ErrorAction SilentlyContinue
}
