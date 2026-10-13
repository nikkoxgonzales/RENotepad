# Builds the release assets into dist\:
#   RENotepad-<version>-setup.exe     installer (Inno Setup)
#   RENotepad-<version>-portable.zip  the exe + license, no install
#   SHA256SUMS.txt
# Inno Setup is fetched on first run (pinned version, hash-checked) and kept
# in .tools\innosetup in portable mode: nothing is installed system-wide.
# The release workflow (.github/workflows/release.yml) runs this same script.
# Usage: pwsh scripts/package.ps1 [-SkipBuild]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if ($env:GITHUB_REF_TYPE -eq 'tag' -and $env:GITHUB_REF_NAME -ne "v$version") {
  throw "Tag $env:GITHUB_REF_NAME doesn't match Cargo.toml version $version"
}

# ---- Inno Setup
$innoVersion = '7.1.0'
$innoSha256 = '0362A383ED217D4C4239B5933866DD96D3EB2102737DA92F80F6057A4B40DF2F'
$iscc = if ($env:ISCC) { $env:ISCC } else { Join-Path $root '.tools\innosetup\ISCC.exe' }
if (-not (Test-Path $iscc)) {
  $dl = Join-Path $root '.tools\inno-dl'
  New-Item -ItemType Directory -Force $dl | Out-Null
  $setup = Join-Path $dl "innosetup-$innoVersion-x64.exe"
  $tag = 'is-' + $innoVersion.Replace('.', '_')
  Invoke-WebRequest "https://github.com/jrsoftware/issrc/releases/download/$tag/innosetup-$innoVersion-x64.exe" -OutFile $setup
  if ((Get-FileHash $setup -Algorithm SHA256).Hash -ne $innoSha256) { Remove-Item $setup; throw 'Inno Setup download hash mismatch' }
  $p = Start-Process $setup -ArgumentList '/PORTABLE=1', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/CURRENTUSER', '/NORESTART', "/DIR=$(Split-Path $iscc)" -PassThru -Wait
  if ($p.ExitCode -ne 0 -or -not (Test-Path $iscc)) { throw "Inno Setup install failed ($($p.ExitCode))" }
}

# ---- build
if (-not $SkipBuild) {
  $cargo = if (Get-Command cargo -ErrorAction SilentlyContinue) { 'cargo' } else { Join-Path $HOME '.cargo\bin\cargo.exe' }
  & $cargo build --release
  if ($LASTEXITCODE) { throw 'cargo build failed' }
}
$exe = Join-Path $root 'target\release\RENotepad.exe'
$exeVersion = (Get-Item $exe).VersionInfo.ProductVersion
if ($exeVersion -ne $version) { throw "exe says $exeVersion, Cargo.toml says ${version}: rebuild" }

# ---- assets
$dist = Join-Path $root 'dist'
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory $dist | Out-Null

$stage = Join-Path $dist 'portable'
New-Item -ItemType Directory $stage | Out-Null
Copy-Item $exe $stage
Copy-Item LICENSE (Join-Path $stage 'LICENSE.txt')
$zip = Join-Path $dist "RENotepad-$version-portable.zip"
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Remove-Item $stage -Recurse

& $iscc /Q "/DAppVersion=$version" installer\RENotepad.iss
if ($LASTEXITCODE) { throw 'Inno Setup compile failed' }

Get-ChildItem $dist -File | Where-Object Name -ne 'SHA256SUMS.txt' | ForEach-Object {
  "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"
} | Set-Content (Join-Path $dist 'SHA256SUMS.txt') -Encoding utf8NoBOM
Get-ChildItem $dist | ForEach-Object { '{0,10:N0}  {1}' -f $_.Length, $_.Name }
