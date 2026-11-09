# Package the built exe as an MSIX for the Microsoft Store.
#
#   pwsh scripts/build-msix.ps1 -Version 0.5.0 [-Exe path] [-Out dir] [-Sign]
#
# The Store re-signs the package, so the upload stays unsigned. -Sign makes a
# throwaway self-signed certificate (same publisher as the manifest), signs
# the package with it and trusts it on this machine, for sideload testing
# only; it needs an elevated shell for the trust step.
param(
  [Parameter(Mandatory)] [string] $Version,
  [string] $Exe = "src-tauri/target/release/dev-cleaner.exe",
  [string] $Out = "dist-msix",
  [switch] $Sign
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

# The Store wants major.minor.build.0; pre-release suffixes are dropped.
$core = ($Version.TrimStart('v') -split '[-+]')[0]
$parts = @($core.Split('.') + @('0', '0', '0'))[0..2]
$msixVersion = ($parts -join '.') + '.0'

function Find-SdkTool([string] $name) {
  $cmd = Get-Command $name -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  $kits = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
  $hit = Get-ChildItem $kits -Recurse -Filter $name -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $hit) { throw "$name not found: install the Windows 10/11 SDK." }
  $hit.FullName
}

if (-not (Test-Path $Exe)) { throw "No exe at ${Exe}: run 'npm run tauri build' first." }

$stage = Join-Path $Out "stage"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory "$stage/Assets" -Force | Out-Null
Copy-Item $Exe "$stage/Dev Cleaner.exe"
foreach ($a in "StoreLogo", "Square44x44Logo", "Square71x71Logo", "Square150x150Logo") {
  Copy-Item "src-tauri/icons/$a.png" "$stage/Assets/$a.png"
}
(Get-Content "packaging/msstore/AppxManifest.xml" -Raw).Replace("{{VERSION}}", $msixVersion) |
  Set-Content "$stage/AppxManifest.xml" -Encoding utf8

$msix = Join-Path $Out "Dev.Cleaner_${core}_x64.msix"
& (Find-SdkTool "makeappx.exe") pack /o /d $stage /p $msix
if ($LASTEXITCODE) { throw "makeappx failed" }

if ($Sign) {
  $publisher = ([xml](Get-Content "$stage/AppxManifest.xml")).Package.Identity.Publisher
  $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher -KeyUsage DigitalSignature `
    -FriendlyName "Dev Cleaner MSIX test" -CertStoreLocation Cert:\CurrentUser\My `
    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
  & (Find-SdkTool "signtool.exe") sign /fd SHA256 /sha1 $cert.Thumbprint $msix
  if ($LASTEXITCODE) { throw "signtool failed" }
  $cer = Join-Path $Out "test-cert.cer"
  Export-Certificate -Cert $cert -FilePath $cer | Out-Null
  Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null
  Write-Host "Signed for testing. Install with: Add-AppxPackage '$msix'"
}
Write-Host "Built $msix (version $msixVersion)"
