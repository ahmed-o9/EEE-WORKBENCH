# Architecture

## Boundaries

```text
Svelte UI (SVG schematic, properties, plots, RAM status)
                  │ Tauri invoke commands
                  ▼
Desktop command bridge (DTO validation and serialization)
        │                 │                 │
        ▼                 ▼                 ▼
Circuit Studio       Project crate     Resource manager
(wire graph → MNA)   (.eeproj)         (RAM + estimates)
        │
        ▼
Numerical crate (MNA, Newton diode, backward Euler, dense LU)
        │
        ▼
Units crate / Shared model graph
```

## Workspace policy

The root workspace contains the five reusable engineering crates and the Circuit Studio conversion crate. The Tauri application is excluded from that workspace because it brings platform SDK, build-script and UI-related dependencies. It still consumes the reusable crates through path dependencies. This preserves a small `cargo test --workspace` target while allowing the app to be built with the Tauri CLI.

## Circuit connectivity

Each two-terminal component has terminal A and terminal B. A ground symbol has only terminal A. A wire joins two terminal references. Circuit Studio validates component IDs and terminal references, joins connected pins using a disjoint-set structure, unifies all ground symbols, and assigns stable one-based node IDs in schematic order. The generated `eee_numerical::Circuit` is then validated again before any simulation.

No assumption is made that an unconnected pin is ground. Floating nets normally surface as a singular MNA matrix with a linear-solver diagnostic.

## Numerical boundary

The numerical crate does not depend on UI state, Tauri, Python, external BLAS or native GPU libraries. `Circuit` owns SI-valued device parameters. `System` owns the assembled dense coefficient matrix and right-hand side. A `LinearSolver` trait is the future replacement point for sparse direct solvers.

## Project boundary

`eee-project` owns the manifest format and its strict JSON parser. Studio content remains an opaque JSON value at this layer, preventing the project crate from taking dependencies on every studio. The desktop bridge serializes/deserializes the circuit DTO and stores it under `studio_data`. The initial UI stores the last DC and transient result objects in the manifest `results` field. Large transient arrays can make the JSON manifest large; compressed binary sidecar storage is a planned format improvement, and manifests are capped at 64 MiB.

## Resource policy

The current resource manager samples available physical RAM on Windows and Linux and reports logical CPU count. It estimates dense matrix and result-buffer memory before simulation. The recommendation uses 35% of currently available physical RAM as a working budget, leaving capacity for the OS, WebView, editor and other programs. This is a conservative estimate, not a hard quota or a guarantee of runtime. Instantaneous CPU utilization and dedicated GPU memory are not currently reported.

## Rendering

The editor uses SVG for schematic geometry and a lightweight SVG polyline for plot traces. This avoids a required WebGL renderer for the first circuit tool. The app reports whether WebGL2 is available for future 3D studios; no WebGPU path is required.
