$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

if (-not (Test-Path (Join-Path $repoRoot 'Cargo.lock'))) {
    throw 'The root Cargo.lock is missing.'
}
if (-not (Test-Path (Join-Path $repoRoot 'apps/desktop/src-tauri/Cargo.lock'))) {
    throw 'apps/desktop/src-tauri/Cargo.lock is missing. Generate it with cargo check on a network-enabled machine and commit it.'
}

Write-Host 'Checking Rust workspace formatting...'
cargo fmt --all -- --check

Write-Host 'Checking the engineering workspace...'
cargo check --workspace --locked

Write-Host 'Running engineering numerical and persistence tests...'
cargo test --workspace --locked

Write-Host 'Checking Tauri desktop Rust formatting and command bridge...'
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
