param(
    [ValidateSet('check','test','release','verify')][string]$Mode='release',
    [ValidateRange(1,64)][int]$Jobs=6,
    [string]$GitBin=''
)
$ErrorActionPreference='Stop'
$rustRoot=Split-Path $PSScriptRoot -Parent
$llvm=Join-Path $env:ProgramFiles 'LLVM'
$python=Join-Path $env:ProgramFiles 'Python312'
$ninja=Join-Path $env:LOCALAPPDATA 'Programs/Ninja'
$cargo=Join-Path $env:USERPROFILE '.cargo/bin'
$env:Path="$cargo;$llvm\bin;$python;$ninja;" + [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User') + ';' + $env:Path
if($GitBin){$env:Path="$GitBin;"+$env:Path}
Get-Command git -ErrorAction Stop | Out-Null
$env:LLVM_HOME=$llvm
$env:LIBCLANG_PATH=Join-Path $llvm 'bin'
$env:CLANG_PATH=Join-Path $llvm 'bin/clang.exe'
$env:CARGO_BUILD_JOBS="$Jobs"
$env:CARGO_HTTP_MULTIPLEXING='false'
# Apply long-path support to child Git processes without changing global config.
$gitCount=if($env:GIT_CONFIG_COUNT){[int]$env:GIT_CONFIG_COUNT}else{0}
[Environment]::SetEnvironmentVariable("GIT_CONFIG_KEY_$gitCount",'core.longpaths','Process')
[Environment]::SetEnvironmentVariable("GIT_CONFIG_VALUE_$gitCount",'true','Process')
$env:GIT_CONFIG_COUNT=[string]($gitCount+1)
$proxy=[Net.WebRequest]::GetSystemWebProxy().GetProxy([uri]'https://github.com')
if($proxy.Host -ne 'github.com'){
    $env:HTTPS_PROXY=$proxy.GetLeftPart([UriPartial]::Authority)
    $env:HTTP_PROXY=$env:HTTPS_PROXY
}
# Keep Rust's official download host on the verified direct route.
$env:NO_PROXY='localhost,127.0.0.1,static.rust-lang.org'
Push-Location $rustRoot
try {
    if($Mode -eq 'verify'){
        cargo fmt --all -- --check
        if($LASTEXITCODE -ne 0){throw 'Formatting failed'}
        & (Join-Path $PSScriptRoot 'build.ps1') -Mode release
        cargo test --release --package i-slint-renderer-skia --lib --locked
        if($LASTEXITCODE -ne 0){throw 'Native Skia tests failed'}
        cargo test --release --workspace --locked -- --test-threads=1
        if($LASTEXITCODE -ne 0){throw 'Workspace tests failed'}
    } else {
        & (Join-Path $PSScriptRoot 'build.ps1') -Mode $Mode
    }
} finally {Pop-Location}
