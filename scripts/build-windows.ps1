$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$required = @('cargo', 'rustc', 'rustfmt', 'node', 'npm')
foreach ($tool in $required) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Required build tool '$tool' was not found. See README.md for prerequisites."
    }
}

if (-not (Test-Path (Join-Path $repoRoot 'Cargo.lock'))) {
    throw 'The root Cargo.lock is missing. Restore the committed workspace lockfile before building.'
}
if (-not (Test-Path (Join-Path $repoRoot 'apps/desktop/src-tauri/Cargo.lock'))) {
    throw 'apps/desktop/src-tauri/Cargo.lock is missing. Generate it online with cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml, review it, and commit it before a reproducible build.'
}

Write-Host 'Validating the engineering workspace...'
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked

Write-Host 'Checking the Tauri desktop Rust command bridge...'
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked

Set-Location (Join-Path $repoRoot 'apps/desktop')
if (-not (Test-Path 'package-lock.json')) {
    throw 'package-lock.json is missing. Generate it online with npm install --package-lock-only, review it, and commit it before running the reproducible build.'
}

Write-Host 'Installing exact frontend dependencies from package-lock.json...'
npm ci

Write-Host 'Checking and building the Svelte frontend...'
npm run check
npm run build

Write-Host 'Building Windows NSIS and MSI installers...'
npm run tauri -- build --bundles nsis,msi

Write-Host 'Build command completed. Verify the installer files under src-tauri/target/release/bundle.'
