# Third-party notices and license register

No third-party numerical engine is bundled. The engineering-core crates use only the Rust standard library and local workspace crates.

| Dependency | Used by | License family / register note |
|---|---|---|
| Tauri 2 | Desktop shell | MIT OR Apache-2.0 (confirm exact resolved crate versions) |
| Tauri CLI 2 | Desktop build tooling | MIT OR Apache-2.0 (confirm exact package metadata) |
| `tauri-plugin-dialog` 2 | Native open/save dialogs | MIT OR Apache-2.0 (confirm exact resolved crate version) |
| Svelte 5 | UI framework | MIT |
| Vite 6 | Frontend build tooling | MIT |
| TypeScript 5 | Type checker/compiler | Apache-2.0 |
| `@tauri-apps/api` 2 | Tauri frontend API | MIT OR Apache-2.0 |
| `@tauri-apps/plugin-dialog` 2 | Native file dialogs | MIT OR Apache-2.0 |
| Serde 1 / serde_json 1 | Desktop DTO serialization | MIT OR Apache-2.0 |

Version ranges are declared in `apps/desktop/package.json` and `apps/desktop/src-tauri/Cargo.toml`. The current environment could not complete dependency installation, so there is no generated lockfile and no exhaustive transitive-license inventory. Before distributing a release, generate and commit `package-lock.json` and `Cargo.lock`, inspect the exact resolved licenses, and include their notices. No proprietary engine or non-free native solver is included.
