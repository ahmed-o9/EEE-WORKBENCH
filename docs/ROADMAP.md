# Feature roadmap and current status

Status meanings: **Implemented** = source exists in the current tree; **Test pending** = authored tests exist but have not executed in the current environment; **Partial** = a subset exists; **Planned** = no functional implementation yet.

| Studio / subsystem | Current status | Notes |
|---|---|---|
| SI units | Implemented · test pending | Dimensions, prefixes, engineering notation and explicit unit checks |
| Rust numerical core | Implemented · test pending | Dense LU, linear MNA, Shockley diode DC, backward-Euler R/C/L transient |
| Shared engineering graph | Partial · test pending | Typed components/parameters/connections; broader schema validation needed |
| `.eeproj` | Implemented · test pending | Versioned JSON, atomic save, backup, autosave and recovery; no binary result store yet |
| Resource manager | Partial · test pending | RAM discovery and dense memory estimates; CPU utilization/GPU details not yet available |
| Circuit Studio graph conversion | Implemented · test pending | Explicit terminal wires, ground unification, net IDs and solver conversion |
| Circuit Studio UI | Partial · build pending | SVG component editor, wire tool, properties, DC/transient actions and plots |
| Analog & Semiconductor | Planned | Not implemented |
| Electrical Machines | Planned | Not implemented |
| Power Systems | Planned | Not implemented |
| Electromagnetics | Planned | Not implemented |
| Control Systems | Planned | Not implemented |
| Signal Processing | Planned | Not implemented |
| Embedded & Digital | Planned | Not implemented |
| 3D viewer / FEM | Planned | No 3D or FEM solver in current build |
| Windows installer | Configuration added · build blocked here | Must be built and verified on Windows |
