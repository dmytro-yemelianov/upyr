param(
    [string]$Version = "",
    [string]$InnoCompiler = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$metadataJson = & cargo metadata --no-deps --format-version 1 --manifest-path (Join-Path $Root "Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Could not read Cargo metadata" }
$CargoVersion = (($metadataJson | ConvertFrom-Json).packages | Where-Object name -eq "upyr" | Select-Object -First 1).version
if (-not $CargoVersion) { throw "Could not read the Upyr package version" }
if ($Version -and $Version -ne $CargoVersion) {
    throw "Requested version '$Version' does not match Cargo version '$CargoVersion'"
}
$Version = $CargoVersion

$Dist = Join-Path $Root "dist"
$Stage = Join-Path $Dist "upyr-windows-x86_64-$Version"
$Cli = Join-Path $Root "target\release\upyr.exe"
$Background = Join-Path $Root "target\release\upyr-background.exe"
$Settings = Join-Path $Root "target\release\upyr-settings.exe"

$reportedVersion = (& $Cli --version).Trim()
if ($reportedVersion -ne "upyr $Version") {
    throw "CLI reports '$reportedVersion'; expected 'upyr $Version'"
}

function Invoke-UpyrSign([string]$Path) {
    if (-not $env:UPYR_SIGNTOOL_PATH) { return }
    if (-not $env:UPYR_CERTIFICATE_PATH -or -not $env:UPYR_CERTIFICATE_PASSWORD) {
        throw "Windows signing requires UPYR_CERTIFICATE_PATH and UPYR_CERTIFICATE_PASSWORD"
    }
    & $env:UPYR_SIGNTOOL_PATH sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com /f $env:UPYR_CERTIFICATE_PATH /p $env:UPYR_CERTIFICATE_PASSWORD $Path
    if ($LASTEXITCODE -ne 0) { throw "Signing failed for $Path" }
    & $env:UPYR_SIGNTOOL_PATH verify /pa /all $Path
    if ($LASTEXITCODE -ne 0) { throw "Signature verification failed for $Path" }
}

Invoke-UpyrSign $Cli
Invoke-UpyrSign $Background
Invoke-UpyrSign $Settings
Remove-Item $Stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item $Stage -ItemType Directory -Force | Out-Null
Copy-Item $Cli $Stage
Copy-Item $Background $Stage
Copy-Item $Settings $Stage
Copy-Item (Join-Path $Root "LICENSE"), (Join-Path $Root "README.md"), (Join-Path $Root "THIRD_PARTY_NOTICES.md") $Stage

$Zip = "$Stage.zip"
Remove-Item $Zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path $Stage -DestinationPath $Zip -CompressionLevel Optimal
if (-not (Test-Path $Zip)) { throw "Portable archive output was not created" }

if (-not (Test-Path $InnoCompiler)) { throw "Inno Setup compiler was not found at $InnoCompiler" }
& $InnoCompiler "/DAppVersion=$Version" "/DSourceDir=$(Join-Path $Root 'target\release')" "/DOutputDir=$Dist" (Join-Path $PSScriptRoot "upyr.iss")
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }

$Installer = Join-Path $Dist "upyr-windows-x86_64-$Version-setup.exe"
if (-not (Test-Path $Installer)) { throw "Installer output was not created" }
Invoke-UpyrSign $Installer
