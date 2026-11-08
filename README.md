# EEE Workbench

**EEE Workbench** is a Windows-first, modular electrical and electronics engineering workbench. This repository contains a dependency-light Rust engineering core and an initial Tauri 2 + Svelte desktop shell.

> **Current scope:** Circuit Studio is the first vertical slice. It supports resistor, capacitor, inductor, DC voltage/current source, Shockley diode and ground symbols; wire-to-net resolution; DC operating point; backward-Euler transient analysis; voltage plots; and `.eeproj` persistence. The other seven studios are roadmap items, not implemented features.

> **Build status:** Milestone 1.1 contains a source-level diode Newton correction, expanded analytical test cases, and command-bridge tests. Numerical validation and desktop compilation remain **unverified** because this environment has no Rust toolchain and cannot resolve external package registries. Do not treat the new tests as passed until the validation commands run successfully on a provisioned machine. See [`docs/VALIDATION_REPORT.md`](docs/VALIDATION_REPORT.md).

## Repository layout

```text
eee-workbench/
├── crates/
│   ├── units/              # SI dimensions and engineering notation
│   ├── numerical/          # Dense LU, MNA, diode Newton iteration, transient solver
│   ├── model/              # Shared typed engineering graph
│   ├── project/            # .eeproj manifest, atomic save, autosave and recovery
│   └── resource-manager/   # RAM detection and simulation memory estimates
├── studios/
│   └── circuit/            # Schematic wire graph → solver circuit conversion
├── apps/desktop/
│   ├── src/                # Svelte/TypeScript circuit editor UI
│   └── src-tauri/          # Tauri 2 shell and Rust command bridge
├── docs/
└── scripts/
```

The root Cargo workspace intentionally excludes `apps/desktop/src-tauri`. This keeps the engineering crates dependency-light and testable without compiling Tauri or its platform SDK. The desktop crate depends on those same path crates and is built separately by the Tauri CLI.

## Core capabilities in this version

- SI dimensions, prefix conversion, and engineering notation parsing (`4.7k`, `100nF`, `12mV`).
- Dense LU with partial pivoting and explicit singular/non-finite diagnostics.
- DC MNA with resistors, independent sources, ideal DC inductors, open DC capacitors, and Newton-linearized Shockley diodes.
- Backward-Euler transient analysis for R/C/L devices and diodes, with per-device capacitor-voltage and inductor-current initial conditions available in the Rust API.
- Circuit schematic connectivity resolved from explicit terminal-to-terminal wires. Unconnected pins are not silently grounded.
- A compact JSON `.eeproj` manifest with schema version, metadata, studio data, simulation settings and results field. Save uses a unique temporary file and retains a `.bak`; autosaves are separate `.autosave` files, with recovery also checking the previous autosave backup if replacement was interrupted.
- RAM discovery on Windows and Linux, and conservative dense-solver memory estimates.
- A Svelte SVG editor that places, moves, edits, wires and deletes circuit components. Its DC and transient actions invoke the Rust solver through Tauri; no simulated waveform is hardcoded.

## Numerical conventions and limits

- Node `0` is ground. Other node identifiers are one-based.
- Device current is positive from terminal A (`a`) to terminal B (`b`). Voltage is `V(a) - V(b)`.
- Voltage-source and inductor branch currents are solved as MNA unknowns. Diode current is evaluated from the Shockley equation after convergence.
- Transient analysis uses backward Euler and zero initial storage states unless the caller supplies initial conditions through the Rust API.
- The transient engine rejects requests exceeding 1,000,000 time steps. The final interval is shortened to land on `t_stop` exactly.
- The first implementation uses a dense LU matrix. Large circuits therefore have quadratic storage and cubic solve cost; it is a small/medium educational and prototyping solver, not yet a sparse SPICE replacement.
- No temperature-dependent, parasitic, reverse-breakdown, charge-storage, or manufacturer-specific semiconductor model is implied by the simple Shockley diode.

See [Numerical methods](docs/NUMERICAL_METHODS.md) and [Validation report](docs/VALIDATION_REPORT.md) for the specific equations and test cases.

## Build on Windows 10/11 x64

### Prerequisites

1. Windows 10/11 x64.
2. Rust stable MSVC toolchain (`rustup`), including `rustfmt`.
3. Visual Studio 2022 Build Tools with **Desktop development with C++** and the Windows SDK.
4. Node.js LTS and npm.
5. Microsoft Edge WebView2 Runtime (normally present on current Windows 10/11 installations; install it if Tauri reports it missing).

Install Rust and Node.js, then open a new PowerShell terminal:

```powershell
winget install --id Rustlang.Rustup -e
winget install --id OpenJS.NodeJS.LTS -e
rustup default stable-x86_64-pc-windows-msvc
rustup component add rustfmt
```

Install Visual Studio Build Tools separately and select the C++ workload. Restart the terminal after installing tools.

### Validate the Rust engineering workspace

From the repository root:

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

The root `Cargo.lock` pins the local, standard-library-only engineering workspace. The desktop Tauri crate is intentionally excluded from that workspace and has third-party Rust dependencies; run the manifest-path check once on a network-enabled build machine and commit the generated `apps/desktop/src-tauri/Cargo.lock` before treating desktop dependency resolution as reproducible. The frontend `package-lock.json` must likewise be generated and committed with `npm install --package-lock-only` on a machine that can reach the npm registry. Do not fabricate lockfile entries when registry metadata is unavailable.

### Run the desktop application in development

```powershell
Set-Location apps/desktop
npm ci
npm run check
npm run dev
```

In a second terminal:

```powershell
Set-Location apps/desktop
npm run tauri -- dev
```

### Build Windows installers

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
Set-Location apps/desktop
npm ci
npm run check
npm run build
npm run tauri -- build --bundles nsis,msi
```

Tauri will compile the separate `src-tauri` crate and use `src-tauri/tauri.conf.json` to package the frontend. The configured bundle targets are NSIS and MSI. The installer is produced only on a Windows machine with its platform dependencies installed; this repository does not include a prebuilt `.exe` or installer.

Helper scripts are provided in [`scripts/`](scripts/).

## Linux development notes

The core crates are standard-library-only and are designed to build independently on Linux, Windows and macOS. The desktop Tauri shell may need platform development packages for WebKitGTK and appindicator integration on Linux. Windows installer output should be built on Windows; do not cross-label a Linux binary as a Windows build.

## First circuit to try

Open [`examples/voltage-divider.eeproj`](examples/voltage-divider.eeproj) from the desktop app. It contains a 10 V source and two 1 kΩ resistors. Run **DC operating point**; the ideal result is 10 V at the source node and 5 V at the midpoint, with 5 mA through each resistor. This file contains the schematic and setup only—no canned simulation result.

## `.eeproj` files

A project file is a UTF-8 JSON manifest with format marker `eeproj` and `schema_version: 1`. It preserves metadata, active studio, studio-specific data, simulation configuration and a results field. Large arrays should be written as compressed binary sidecar assets in a future schema; they should not be expanded into uncompressed JSON. The first UI stores the circuit element/wire graph, transient settings and most recent DC/transient results in the manifest. Saving is capped at 64 MiB to prevent files that the reader cannot reopen.

The Rust project API supports `Project::save`, `Project::autosave`, `Project::load`, and `Project::load_with_recovery`. The desktop app persists its schematic, simulation settings, and last DC/transient results in the manifest. Saving an existing project preserves its original creation timestamp. Recovery picks the newest valid primary/autosave/backup manifest and reports which copy was loaded. Manifests are currently limited to 64 MiB; a compressed binary result store is planned for larger studies.

## License

This repository is provided under the MIT License. Third-party packages remain under their own licenses; see [Third-party notices](THIRD_PARTY_NOTICES.md). The root Rust workspace lockfile contains only local crates, and therefore has no third-party Rust package versions to pin. The separate Tauri desktop crate and the npm frontend still require registry-generated lockfiles; generation was blocked in the current offline environment and is explicitly recorded in `docs/VALIDATION_REPORT.md`.
