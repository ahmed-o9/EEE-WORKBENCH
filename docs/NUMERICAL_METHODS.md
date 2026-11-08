# Numerical methods and sign conventions

## Node and current convention

Ground is node 0. All other node IDs are one-based. Device voltage is `v = V(a) - V(b)`, and positive device current flows from A to B. The assembled nodal system is `G x = b`, with unknowns ordered as node voltages followed by branch currents.

## Linear devices

A resistor stamps `g = 1/R` into the four conductance entries for its two terminals. A positive independent current source from A to B contributes `-I` to the A-node right-hand side and `+I` to the B-node right-hand side. An ideal voltage source adds a branch-current unknown and the equation `V(a) - V(b) = Vs`. An inductor is a zero-volt branch in DC. A capacitor is open in DC.

Branch-current unknowns are only allocated for voltage sources and inductors. A diode is not a voltage-defined branch and must not receive a redundant ideal branch equation.

## Shockley diode

The model is `I = Is * (exp(v/Vt) - 1)`. Around the current junction-voltage estimate `v0`, the tangent is `I ≈ g_eq * v + I_eq`, where `g_eq = Is * exp(v0/Vt) / Vt` and `I_eq = I(v0) - g_eq * v0`. The conductance is stamped between A and B and the intercept is stamped as a current source from A to B. Forward-going Newton updates are damped to at most 0.1 V because the forward exponential is stiff; reverse-going updates are accepted without an artificial reverse-voltage floor. The exponential argument is bounded to `[-80, 40]` for numerical safety. This means the computed reverse current approaches `-Is`; reverse breakdown is not represented. This is a basic educational Shockley model, not a complete SPICE diode model.

## Backward-Euler capacitor

For step size `h`, `i[n] = (C/h) * (v[n] - v[n-1])`. Its companion is a conductance `C/h` and an equivalent current source `-(C/h) * v[n-1]` directed A to B. The source sign is important: it must preserve the previous capacitor voltage when combined with the conductance.

## Backward-Euler inductor

For an inductor branch current `i[n]`, `v[n] = (L/h) * (i[n] - i[n-1])`. The MNA branch equation is `V(a) - V(b) - (L/h)i[n] = -(L/h)i[n-1]`. This uses the same branch-current direction as the component definition.

## Time stepping

The first transient API uses zero initial capacitor voltages and inductor currents by default. `TransientInitialConditions` allows optional values indexed by device order. The initial sample is obtained from an algebraic MNA solve that constrains capacitor voltages and inductor currents to those initial states; contradictory ideal-source/storage constraints fail as singular systems. Only converged steps commit storage state. The final step is shortened so the final time equals the requested `t_stop`. A cap of 1,000,000 points prevents accidental allocation of unbounded result arrays.

## Solver limitations

- Dense LU is used; memory grows as O(n²), arithmetic as O(n³).
- No sparse matrix backend yet.
- No adaptive time-step control, trapezoidal integration, Gear methods or local truncation-error estimator.
- No periodic/PWL/time-dependent source definitions in this vertical slice.
- No capacitor ESR/leakage, inductor winding resistance/core saturation, mutual inductance, thermal coupling or parasitics.
- Diode convergence uses a bounded exponential and forward-step damping. Reverse current is limited by the Shockley model (approximately `-Is`); reverse breakdown, leakage mechanisms beyond `Is`, series resistance, temperature dependence and self-heating are not represented. Pathological nonlinear networks may still return `NoConvergence`.
- A singular matrix is reported; automatic gmin stepping, shunt insertion or topology repair is not performed.
