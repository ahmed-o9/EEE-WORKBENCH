# Milestone 1.1 validation report

**Report date:** 2026-11-07  
**Source:** continuation of the supplied EEE Workbench 0.1.0 repository  
**Environment:** Debian GNU/Linux 13 x86_64; Node.js v22.16.0; npm 10.9.2  
**Toolchain availability:** `cargo`, `rustc`, `rustfmt`, PowerShell and the frontend `node_modules` tree are absent.

## Executive result

Milestone 1.1 contains source corrections and additional tests for the numerical solver, transient storage-state equations, project recovery and the Tauri command boundary. **No Rust or frontend test was executed successfully in this environment.** Do not treat this milestone as numerically validated or as a compiled desktop release until the commands in this report pass on a provisioned machine.

The root `Cargo.lock` is present and covers the six local packages in the dependency-light root workspace. It was authored from the six local manifests and independently TOML-parsed; because Cargo was absent, it is **not Cargo-generated or Cargo-verified**. It is an intentionally local-only graph: `eee-model` depends on `eee-units`, and `eee-circuit-studio` depends on `eee-numerical`; the remaining local crates have no third-party dependencies. The Tauri crate is excluded from that workspace and the frontend has external packages. Their separate lockfiles could not be resolved without registry access, so **`apps/desktop/src-tauri/Cargo.lock` and `apps/desktop/package-lock.json` are still outstanding**. No guessed or incomplete package lock was added.

## Changes made

### Diode Newton iteration

- Removed the artificial `-0.5 V` diode-voltage guess floor from both DC and transient operating-point iterations.
- Centralized the Shockley current calculation and tangent stamping in `crates/numerical/src/mna.rs`; DC results, transient initial samples, transient step-current results and all diode tangent stamps now use the shared implementation.
- Reverse-going voltage updates are accepted in full. Forward-going updates remain damped to at most `0.1 V` per iteration to control the stiff forward exponential.
- Retained the exponential argument guard `[-80, 40]` for floating-point safety.
- Kept the stated model limitations: no reverse breakdown, series resistance, temperature dependence, self-heating or complete SPICE diode effects. Reverse current approaches `-Is` in this model.
- Added DC tests for `-12 V` reverse bias and several forward-bias source/resistance/Is/Vt combinations; tests check calculated current against the Shockley equation and check node KCL.
- Added transient tests for forward and reverse bias; tests check current sign, the Shockley equation and KCL at every saved time sample.
- Corrected a pre-existing test reference-range error. With `Is = 2.52e-9 A`, `Vt = 0.02585 V`, a 5 V source and 1 kΩ load, the Shockley operating voltage is about `0.37284 V`, not within the old `0.5–0.8 V` assertion. The test now asserts the compatible range while still checking the Shockley current and KCL.

### Capacitor and inductor transient equations

The documented signs and code stamps are consistent with the component current reference direction A-to-B:

- Capacitor: `i[n] = (C/h) * (v[n] - v[n-1])`; Norton source is `-(C/h) * v[n-1]` directed A-to-B.
- Inductor: `v[n] = (L/h) * (i[n] - i[n-1])`; branch equation is `V(a)-V(b)-(L/h)i[n] = -(L/h)i[n-1]`.
- Non-zero capacitor initial voltage is checked with an analytical RC discharge response, negative capacitor discharge current and initial KCL.
- Non-zero inductor initial current is checked with an analytical RL natural response, current direction, resistor/inductor voltage relation and KCL.
- Transient step current arrays are checked for non-finite results before storage state is committed.
- Added a guard against a time step that cannot advance the floating-point simulation clock.

### Test review and persistence / bridge coverage

- Added a conflicting parallel ideal-voltage-source singularity test.
- Existing tests for singular/floating nodes, bad component values, source polarities, LU pivoting, RC/RL/RLC responses and invalid time steps were reviewed and retained.
- Fixed project test isolation: temporary directories now include an atomic per-test sequence, avoiding collisions between parallel Rust tests in one process.
- Added Tauri command-bridge tests for schematic DTO conversion, deterministic node naming, DC result serialization, transient waveform serialization, project save/reopen, and autosave recovery after corrupting the primary manifest.
- The Svelte result view was manually inspected: it consumes the `nodeVoltages` / `deviceCurrents` fields for DC results and `times` / `traces` for transient plots. Command tests assert these serialized field names and calculated values; the Svelte compiler could not be run here.

### Lockfiles and Windows scripts

- Added root `Cargo.lock` for the local-only workspace graph. The file was TOML-parsed and its package/dependency entries were compared with the workspace manifests. Cargo itself was unavailable, so Cargo did not regenerate or validate this lockfile.
- Attempted offline npm lock generation. It failed with `ENOTCACHED` because the npm cache has zero entries. Online registry access failed DNS resolution (`EAI_AGAIN`). Therefore no valid `package-lock.json` was fabricated.
- Reviewed and updated `scripts/check-core.ps1` to include root formatting/check/tests and the separate Tauri formatting/check commands.
- Updated both PowerShell scripts to require the relevant committed lockfiles and use `--locked` for Cargo checks/tests. The Windows build script requires `package-lock.json`, uses `npm ci`, runs Svelte checks/build, and packages NSIS/MSI only after those gates pass.
- Manually reviewed `tauri.conf.json`: NSIS/MSI targets, PNG/ICO bundle icons, Vite dev URL, `../dist` frontend output and Tauri v2 IPC/dialog capability/CSP settings are internally consistent by inspection.
- The PowerShell scripts, Tauri schema validation and Windows installer configuration could not be executed in this Linux environment.

## Exact requested command results

All requested commands were invoked in this environment. The failures below are environment/tool availability failures, not test failures produced by the source code.

| Command | Exit code | Actual result |
|---|---:|---|
| `cargo fmt --all -- --check` | 127 | `cargo: command not found` |
| `cargo check --workspace` | 127 | `cargo: command not found` |
| `cargo test --workspace` | 127 | `cargo: command not found` |
| `cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml` | 127 | `cargo: command not found` |
| `npm run check` | 127 | `svelte-check: not found` |
| `npm run build` | 127 | `vite: not found` |

Dependency and toolchain acquisition attempts:

| Attempt | Result |
|---|---|
| `curl -I https://static.rust-lang.org` | DNS failure: could not resolve host |
| `npm view svelte@5.20.0 version` | DNS failure: `EAI_AGAIN registry.npmjs.org` |
| `npm install --package-lock-only --ignore-scripts --offline --no-audit --no-fund` | Exit 1: `ENOTCACHED`, no cached response for `@sveltejs/vite-plugin-svelte` |
| npm cache verification | 0 cached content entries |
| Search for `cargo`, `rustc`, `rustfmt` in standard runtime paths | None found |

## Checks completed without the Rust/npm toolchains

The following are static/source checks only. They do not establish that Rust compiles, that Cargo accepts the lockfile, that Svelte templates type-check, or that the frontend bundles.

| Check | Result | Scope |
|---|---|---|
| Repository inventory | Completed | Reviewed the supplied Milestone 1 source tree and manifests |
| All Cargo manifest + root Cargo.lock parse | Passed | Python TOML parser; local package graph compared with manifests |
| Tauri / capability / frontend / example JSON config parse | Passed | Python JSON parser only; not a Tauri schema validation |
| Vite and Svelte config JS syntax | Passed | Node syntax checks only; Svelte templates were not compiled |
| Rust-source delimiter scan | Passed | 13 `.rs` files; custom lexical bracket scan only, not a Rust parser/compiler |
| Source-level numerical sign review | Manually inspected | Not a substitute for Rust tests |
| Python equivalent diode MNA spot checks | Passed | Independent arithmetic model only; not the Rust implementation |
| Python backward-Euler RC/RL spot checks | Passed | Independent discrete-equation checks only; not the Rust implementation |
| Tauri command tests | Authored, not executed | Requires Cargo and Tauri dependency resolution |
| Frontend result display | Manually inspected | Svelte templates not compiled; data-field mapping reviewed in source |
| Windows scripts / NSIS / MSI | Not executed | Linux runtime, no PowerShell, no Windows toolchain |

Independent arithmetic spot checks of the revised Newton update produced the following operating points (these are **not Rust test results**):

| Circuit | Diode voltage | Diode current | Newton iterations | KCL residual |
|---|---:|---:|---:|---:|
| -12 V, 4.7 kΩ reverse path, `Is=2.52 nA` | -11.999988 V | -2.520 nA | 3 | approximately 0 A |
| 5 V, 1 kΩ, `Is=2.52 nA` | 0.372840 V | 4.627 mA | 10 | below 4e-17 A |
| 3.3 V, 2.2 kΩ, `Is=1 pA` | 0.541544 V | 1.254 mA | 13 | below 7e-18 A |
| 12 V, 4.7 kΩ, `Is=1 nA` | 0.382731 V | 2.472 mA | 9 | below 5e-17 A |

The equivalent backward-Euler spot checks gave `1.84032 V` for a 5 V, 1 kΩ / 1 µF RC discharge at one time constant (continuous analytical reference `1.83940 V`) and `0.368063 A` for a 1 A, 10 Ω / 0.1 H RL natural response after one time constant (continuous analytical reference `0.367879 A`).

## Feature verification status

| Feature | Implemented in source | Compiled | Tests executed | Current classification |
|---|---:|---:|---:|---|
| Root workspace crates and local lockfile | Yes | No | No | Manually inspected; compile unverified |
| DC MNA and diode Newton reverse handling | Yes | No | No | Tests authored; numerically unverified in Rust |
| Capacitor / inductor backward Euler | Yes | No | No | Analytical tests authored; numerically unverified in Rust |
| Singular / floating / conflicting-source errors | Yes | No | No | Tests authored; unverified |
| Schematic-to-solver net conversion | Yes | No | No | Existing tests plus bridge tests authored; unverified |
| Tauri command bridge | Yes | No | No | Manually inspected; unverified |
| Project save/reopen and recovery | Yes | No | No | Existing tests plus command tests authored; unverified |
| Svelte DC/transient result display | Yes | No | No | Manually inspected; frontend build unverified |
| Tauri Rust dependency lockfile | No | No | No | Blocked by unavailable crates.io access |
| npm package lockfile | No | No | No | Blocked by unavailable npm registry/cache |
| Windows installer | Configuration only | No | No | Not built; Windows verification outstanding |

## Required follow-up on a network-enabled machine

This environment could not complete the core milestone gates. The root workspace lockfile is a local-only hand-authored lock graph, not an observed Cargo resolution. The independent Tauri crate and frontend do not yet have committed dependency lockfiles. Before describing the desktop dependency graph as reproducible, generate those two lockfiles with the real registries and commit them.

From the repository root, generate and commit the registry-backed lockfiles before calling the build reproducible:

```powershell
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
Set-Location apps/desktop
npm install --package-lock-only
Set-Location ../..
```

Then run the complete requested verification set and the additional Tauri command-bridge tests:

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml

Set-Location apps/desktop
npm ci
npm run check
npm run build
```

Finally, on Windows with Visual Studio C++ Build Tools and WebView2 available:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
npm run tauri -- build --bundles nsis,msi
```

Record the generated Tauri and npm lockfiles in version control, inspect the installer output under `apps/desktop/src-tauri/target/release/bundle`, and rerun the project/bridge tests. No Windows executable or installer is included in this source archive.
