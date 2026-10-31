param([ValidateSet('check','test','release')][string]$Mode='release')
$ErrorActionPreference='Stop'
$rustRoot=Split-Path $PSScriptRoot -Parent
if(-not (Get-Command cargo -ErrorAction SilentlyContinue)){throw 'Install Rust with rustup before building.'}
if(-not (Get-Command rc.exe -ErrorAction SilentlyContinue)){
    $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if(-not (Test-Path -LiteralPath $vswhere)){throw 'MSVC and Windows SDK are required. Use Developer PowerShell or install Visual Studio C++ Build Tools.'}
    $installation=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if(-not $installation){throw 'No Visual Studio installation with MSVC x64 tools was found.'}
    & (Join-Path $installation 'Common7/Tools/Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
}
if(-not (Get-Command rc.exe -ErrorAction SilentlyContinue)){throw 'Windows SDK resource compiler rc.exe is missing.'}
$arguments=switch($Mode){
    check { @('check','--workspace','--all-targets','--locked') }
    test { @('test','--workspace','--locked','--','--test-threads=1') }
    release { @('build','--release','--locked') }
}
Push-Location $rustRoot
try {
    & cargo @arguments
    if($LASTEXITCODE -ne 0){throw "Cargo $Mode failed with exit code $LASTEXITCODE"}
} finally {Pop-Location}
