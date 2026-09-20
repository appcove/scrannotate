#requires -Version 5.1
<#
.SYNOPSIS
Build an unsigned Store MSIX from a prepared release executable and real assets.
.DESCRIPTION
See docs/store-packaging.md. Identity values must come from Partner Center.
MakeAppx performs schema/semantic validation. This does not submit or install.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][ValidateSet('x64', 'arm64')][string]$Architecture,
    [Parameter(Mandatory = $true)][string]$IdentityName,
    [Parameter(Mandatory = $true)][string]$Publisher,
    [Parameter(Mandatory = $true)][string]$PublisherDisplayName,
    [Parameter(Mandatory = $true)][string]$AssetsDirectory,
    [Parameter(Mandatory = $true)][string]$Notices,
    [Parameter(Mandatory = $true)][string]$MakeAppxPath,
    [Parameter(Mandatory = $true)][string]$MaxVersionTested,
    [Parameter(Mandatory = $true)][string]$Output,
    [string]$DisplayName = 'scrannotate',
    [string]$MinVersion = '10.0.19041.0'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Require-File([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    if ($item.PSIsContainer -or $item.Length -eq 0) { throw "Missing or empty file: $Path" }
    return $item.FullName
}

function Parse-Version([string]$Value) {
    if ($Value -notmatch '^(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})$') {
        throw "Version must have four decimal components without leading zeros: $Value"
    }
    $parts = @($Value.Split('.') | ForEach-Object { [int]$_ })
    if (@($parts | Where-Object { $_ -gt 65535 }).Count -ne 0) {
        throw "Version components must not exceed 65535: $Value"
    }
    return [version]$Value
}

function Require-Png([string]$Path, [int]$Size) {
    $path = Require-File $Path
    $bytes = [IO.File]::ReadAllBytes($path)
    if ($bytes.Length -lt 24 -or [BitConverter]::ToString($bytes, 0, 8) -ne '89-50-4E-47-0D-0A-1A-0A' -or
        [Text.Encoding]::ASCII.GetString($bytes, 12, 4) -ne 'IHDR') {
        throw "Asset is not a PNG: $path"
    }
    $width = [uint32]$bytes[16] * 16777216 + [uint32]$bytes[17] * 65536 + [uint32]$bytes[18] * 256 + $bytes[19]
    $height = [uint32]$bytes[20] * 16777216 + [uint32]$bytes[21] * 65536 + [uint32]$bytes[22] * 256 + $bytes[23]
    if ($width -ne $Size -or $height -ne $Size) { throw "Asset must be ${Size}x${Size}: $path" }
    return $path
}

function Escape-Xml([string]$Value) {
    # Reject characters XML cannot represent; escape values, never concatenate raw XML.
    $null = [Xml.XmlConvert]::VerifyXmlChars($Value)
    return [Security.SecurityElement]::Escape($Value)
}

$packageVersion = Parse-Version $Version
if ($packageVersion.Major -eq 0 -or $packageVersion.Revision -ne 0) {
    throw 'Store version requires a nonzero major version and a fourth component of zero.'
}
$minimum = Parse-Version $MinVersion
$maximum = Parse-Version $MaxVersionTested
if ($minimum -lt [version]'10.0.19041.0' -or $maximum -lt $minimum) {
    throw 'MinVersion must be at least 10.0.19041.0 and MaxVersionTested must be at least MinVersion.'
}
if ($IdentityName -notmatch '^[A-Za-z0-9.-]{3,50}$') { throw 'Invalid Partner Center package identity name.' }
if ([string]::IsNullOrWhiteSpace($Publisher) -or [string]::IsNullOrWhiteSpace($PublisherDisplayName)) {
    throw 'Partner Center publisher and publisher display name are required.'
}
if ([IO.Path]::GetExtension($Output) -ine '.msix') { throw 'Output must have a .msix extension.' }
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Run this script on Windows with the Windows SDK.' }

$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$binaryPath = Require-File $Binary
$noticesPath = Require-File $Notices
$sdkTool = Require-File $MakeAppxPath
if ([IO.Path]::GetFileName($sdkTool) -ine 'makeappx.exe') { throw 'MakeAppxPath must name the Windows SDK makeappx.exe.' }
$documents = @('LICENSE', 'NOTICE', 'PRIVACY.md')
foreach ($document in $documents) { $null = Require-File (Join-Path $root $document) }
$assets = @{
    'StoreLogo.png' = Require-Png (Join-Path $AssetsDirectory 'StoreLogo.png') 50
    'Square44x44Logo.png' = Require-Png (Join-Path $AssetsDirectory 'Square44x44Logo.png') 44
    'Square150x150Logo.png' = Require-Png (Join-Path $AssetsDirectory 'Square150x150Logo.png') 150
}

# Check the executable, rather than trusting the requested architecture.
$reader = [IO.BinaryReader]::new([IO.File]::OpenRead($binaryPath))
try {
    if ($reader.BaseStream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) { throw 'Binary is not a Windows PE executable.' }
    $reader.BaseStream.Position = 0x3c
    $offset = $reader.ReadUInt32()
    if ($offset + 6 -gt $reader.BaseStream.Length) { throw 'Invalid PE header offset.' }
    $reader.BaseStream.Position = $offset
    if ($reader.ReadUInt32() -ne 0x4550) { throw 'Invalid PE signature.' }
    $machine = $reader.ReadUInt16()
    $expected = if ($Architecture -eq 'x64') { 0x8664 } else { 0xaa64 }
    if ($machine -ne $expected) { throw "Binary machine does not match $Architecture." }
} finally { $reader.Dispose() }

# Resolve against PowerShell's current location, which may differ from the
# process working directory after Set-Location.
$destination = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
if (Test-Path -LiteralPath $destination) { throw "Refusing to overwrite $destination" }
$null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
$staging = Join-Path ([IO.Path]::GetTempPath()) ('scrannotate-msix-' + [Guid]::NewGuid().ToString('N'))
try {
    $payload = Join-Path $staging 'payload'
    $assetOutput = Join-Path $payload 'Assets'
    $null = [IO.Directory]::CreateDirectory($assetOutput)
    Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $payload 'scrannotate.exe')
    foreach ($document in $documents) { Copy-Item -LiteralPath (Join-Path $root $document) -Destination $payload }
    Copy-Item -LiteralPath $noticesPath -Destination (Join-Path $payload 'THIRD_PARTY_NOTICES.txt')
    foreach ($asset in $assets.GetEnumerator()) { Copy-Item -LiteralPath $asset.Value -Destination (Join-Path $assetOutput $asset.Key) }

    $nameXml = Escape-Xml $IdentityName
    $publisherXml = Escape-Xml $Publisher
    $publisherDisplayXml = Escape-Xml $PublisherDisplayName
    $displayXml = Escape-Xml $DisplayName
    $manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
 xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
 xmlns:uap5="http://schemas.microsoft.com/appx/manifest/uap/windows10/5"
 xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
 IgnorableNamespaces="uap uap5 rescap">
 <Identity Name="$nameXml" Publisher="$publisherXml" Version="$Version" ProcessorArchitecture="$Architecture" />
 <Properties>
  <DisplayName>$displayXml</DisplayName>
  <PublisherDisplayName>$publisherDisplayXml</PublisherDisplayName>
  <Logo>Assets\StoreLogo.png</Logo>
 </Properties>
 <Dependencies>
  <TargetDeviceFamily Name="Windows.Desktop" MinVersion="$MinVersion" MaxVersionTested="$MaxVersionTested" />
  <PackageDependency Name="Microsoft.VCLibs.140.00.UWPDesktop" MinVersion="14.0.30704.0"
   Publisher="CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US" />
 </Dependencies>
 <Resources><Resource Language="en-US" /></Resources>
 <Applications>
  <Application Id="scrannotate" Executable="scrannotate.exe" EntryPoint="Windows.FullTrustApplication">
   <uap:VisualElements DisplayName="$displayXml" Description="Capture and annotate screenshots"
    BackgroundColor="transparent" Square44x44Logo="Assets\Square44x44Logo.png"
    Square150x150Logo="Assets\Square150x150Logo.png" />
   <Extensions>
    <uap5:Extension Category="windows.appExecutionAlias" Executable="scrannotate.exe" EntryPoint="Windows.FullTrustApplication">
     <uap5:AppExecutionAlias><uap5:ExecutionAlias Alias="scrannotate.exe" /></uap5:AppExecutionAlias>
    </uap5:Extension>
   </Extensions>
  </Application>
 </Applications>
 <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@
    # Parsing catches malformed XML before the SDK performs schema validation.
    $null = [xml]$manifest
    [IO.File]::WriteAllText((Join-Path $payload 'AppxManifest.xml'), $manifest, [Text.UTF8Encoding]::new($false))
    $package = Join-Path $staging 'scrannotate.msix'
    & $sdkTool pack /d $payload /p $package /h SHA256 /no
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed with exit code $LASTEXITCODE." }
    $null = Require-File $package
    # File.Move refuses replacement even if another packager created Output meanwhile.
    [IO.File]::Move($package, $destination)
    Write-Output "Created unsigned Store package: $destination"
} finally {
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
}
