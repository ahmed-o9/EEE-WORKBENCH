//! Modified Nodal Analysis (MNA) assembly and DC operating point.
//!
//! Unknowns are ordered as `[non-ground node voltages; branch currents]`.
//! Voltage sources and inductors have branch-current unknowns. Diodes use a
//! Newton-linearized conductance plus current-source companion and therefore
//! do not need an additional branch equation.

use crate::linalg::{DenseLu, LinearSolver, LuError};

pub type NodeId = usize; // node 0 is ground
const DIODE_MAX_EXPONENT: f64 = 40.0;
const DIODE_MIN_EXPONENT: f64 = -80.0;
const DIODE_MAX_NEWTON_STEP_V: f64 = 0.1;
const NEWTON_TOLERANCE_V: f64 = 1e-9;
const MAX_DIODE_ITERATIONS: usize = 200;

/// Shockley current with an exponent guard against floating-point overflow and
/// underflow. Reverse breakdown, series resistance and self-heating are not
/// part of this idealized diode model.
pub(crate) fn diode_current(is: f64, vt: f64, voltage: f64) -> f64 {
    let exponent = (voltage / vt).clamp(DIODE_MIN_EXPONENT, DIODE_MAX_EXPONENT);
    is * (exponent.exp() - 1.0)
}

/// Stamp the tangent of the Shockley equation at `guess` as a conductance and
/// an A-to-B current source. The diode voltage itself is not clamped: in
/// particular, realistic reverse voltages must remain representable. Only the
/// exponential argument is bounded for numerical safety.
pub(crate) fn stamp_diode_tangent(
    system: &mut System,
    a: NodeId,
    b: NodeId,
    is: f64,
    vt: f64,
    guess: f64,
) {
    let exponent = (guess / vt).clamp(DIODE_MIN_EXPONENT, DIODE_MAX_EXPONENT);
    let exponential = exponent.exp();
    let current = is * (exponential - 1.0);
    let conductance = is * exponential / vt;
    let intercept = current - conductance * guess;
    stamp_conductance(system, a, b, conductance);
    stamp_current_source(system, a, b, intercept);
}

/// Dampen only positive Newton excursions, where the Shockley exponential is
/// stiff. A reverse-going update is accepted in full because the reverse
/// current is bounded by approximately -Is and small fixed steps would make
/// ordinary reverse-bias circuits needlessly fail the iteration limit.
pub(crate) fn next_diode_voltage_guess(previous: f64, solved: f64) -> f64 {
    if solved <= previous {
        solved
    } else {
        previous + (solved - previous).min(DIODE_MAX_NEWTON_STEP_V)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Device {
    Resistor {
        name: String,
        a: NodeId,
        b: NodeId,
        ohms: f64,
    },
    /// V(a) - V(b) = volts (DC source).
    VoltageSource {
        name: String,
        a: NodeId,
        b: NodeId,
        volts: f64,
    },
    /// Positive current flows from a to b.
    CurrentSource {
        name: String,
        a: NodeId,
        b: NodeId,
        amps: f64,
    },
    /// Shockley diode: I = Is * (exp(Vd / Vt) - 1), current a -> b.
    Diode {
        name: String,
        a: NodeId,
        b: NodeId,
        is: f64,
        vt: f64,
    },
    /// Farads; transient companion model (backward Euler).
    Capacitor {
        name: String,
        a: NodeId,
        b: NodeId,
        farads: f64,
    },
    /// Henries; branch-current formulation.
    Inductor {
        name: String,
        a: NodeId,
        b: NodeId,
        henries: f64,
    },
}

/// A mutable MNA system being stamped.
pub struct System {
    pub n: usize,
    pub g: Vec<Vec<f64>>,
    pub b: Vec<f64>,
}

impl System {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            g: vec![vec![0.0; n]; n],
            b: vec![0.0; n],
        }
    }

    pub fn stamp_g(&mut self, row: usize, col: usize, value: f64) {
        self.g[row][col] += value;
    }

    pub fn stamp_b(&mut self, row: usize, value: f64) {
        self.b[row] += value;
    }

    pub fn solve(self) -> Result<Vec<f64>, SolveError> {
        let mut lu = DenseLu::new(self.n);
        lu.a = self.g;
        lu.b = self.b;
        lu.solve().map_err(SolveError::from)
    }
}

#[derive(Debug, PartialEq)]
pub enum SolveError {
    NoConvergence(usize),
    Lu(LuError),
    BadDevice(String),
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::NoConvergence(iterations) => {
                write!(f, "Newton iteration did not converge in {iterations} steps")
            }
            SolveError::Lu(error) => write!(f, "linear solve failed: {error}"),
            SolveError::BadDevice(message) => write!(f, "invalid circuit: {message}"),
        }
    }
}
impl std::error::Error for SolveError {}
impl From<LuError> for SolveError {
    fn from(error: LuError) -> Self {
        SolveError::Lu(error)
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Circuit {
    pub num_nodes: usize, // excludes ground
    pub devices: Vec<Device>,
}

pub struct DcResult {
    /// Node voltages, index 0 is ground.
    pub node_voltages: Vec<f64>,
    /// Signed device currents, in each device's a -> b reference direction.
    pub device_currents: Vec<f64>,
    /// Retained for compatibility with the first solver API.
    pub branch_currents: Vec<f64>,
}

impl Circuit {
    /// Validate terminal references and all physical parameters before stamping.
    pub fn validate(&self) -> Result<(), SolveError> {
        for device in &self.devices {
            let (a, b) = terminals(device);
            for node in [a, b] {
                if node > self.num_nodes {
                    return Err(SolveError::BadDevice(format!(
                        "terminal references node {node}, but circuit has nodes 0..={}",
                        self.num_nodes
                    )));
                }
            }
            match device {
                Device::Resistor { ohms, .. } if !positive_finite(*ohms) => {
                    return Err(SolveError::BadDevice(format!(
                        "resistance must be finite and > 0 (got {ohms})"
                    )));
                }
                Device::VoltageSource { volts, .. } if !volts.is_finite() => {
                    return Err(SolveError::BadDevice(
                        "voltage source must be finite".into(),
                    ));
                }
                Device::CurrentSource { amps, .. } if !amps.is_finite() => {
                    return Err(SolveError::BadDevice(
                        "current source must be finite".into(),
                    ));
                }
                Device::Diode { is, vt, .. } if !positive_finite(*is) || !positive_finite(*vt) => {
                    return Err(SolveError::BadDevice(
                        "diode Is and Vt must both be finite and > 0".into(),
                    ));
                }
                Device::Capacitor { farads, .. } if !positive_finite(*farads) => {
                    return Err(SolveError::BadDevice(format!(
                        "capacitance must be finite and > 0 (got {farads})"
                    )));
                }
                Device::Inductor { henries, .. } if !positive_finite(*henries) => {
                    return Err(SolveError::BadDevice(format!(
                        "inductance must be finite and > 0 (got {henries})"
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn unknown_count(&self) -> usize {
        self.num_nodes + self.extra_unknowns()
    }

    pub(crate) fn extra_unknowns(&self) -> usize {
        self.devices
            .iter()
            .filter(|device| {
                matches!(
                    device,
                    Device::VoltageSource { .. } | Device::Inductor { .. }
                )
            })
            .count()
    }

    pub(crate) fn branch_slots(&self) -> Vec<Option<usize>> {
        let mut next = self.num_nodes;
        self.devices
            .iter()
            .map(|device| {
                if matches!(
                    device,
                    Device::VoltageSource { .. } | Device::Inductor { .. }
                ) {
                    let current = next;
                    next += 1;
                    Some(current)
                } else {
                    None
                }
            })
            .collect()
    }

    /// DC operating point. Capacitors are open and inductors are ideal shorts.
    pub fn dc(&self) -> Result<DcResult, SolveError> {
        self.validate()?;
        let unknowns = self.num_nodes + self.extra_unknowns();
        if unknowns == 0 {
            return Err(SolveError::BadDevice("circuit has no unknowns".into()));
        }

        let mut diode_guess = vec![0.0; self.devices.len()];
        let has_diodes = self
            .devices
            .iter()
            .any(|device| matches!(device, Device::Diode { .. }));
        let slots = self.branch_slots();

        if !has_diodes {
            let mut system = System::new(unknowns);
            self.stamp_base(&mut system, &diode_guess, &slots)?;
            let solution = system.solve()?;
            return self.dc_result(solution, &slots);
        }

        for iteration in 0..MAX_DIODE_ITERATIONS {
            let mut system = System::new(unknowns);
            self.stamp_base(&mut system, &diode_guess, &slots)?;
            let solution = system.solve()?;
            let mut largest_error = 0.0_f64;

            for (index, device) in self.devices.iter().enumerate() {
                if let Device::Diode { a, b, .. } = device {
                    let solved_voltage = node_voltage(&solution, *a) - node_voltage(&solution, *b);
                    let delta = solved_voltage - diode_guess[index];
                    largest_error = largest_error.max(delta.abs());
                    diode_guess[index] =
                        next_diode_voltage_guess(diode_guess[index], solved_voltage);
                }
            }

            if largest_error < NEWTON_TOLERANCE_V {
                return self.dc_result(solution, &slots);
            }
            if iteration + 1 == MAX_DIODE_ITERATIONS {
                break;
            }
        }
        Err(SolveError::NoConvergence(MAX_DIODE_ITERATIONS))
    }

    pub(crate) fn stamp_base(
        &self,
        system: &mut System,
        diode_guess: &[f64],
        slots: &[Option<usize>],
    ) -> Result<(), SolveError> {
        for (index, device) in self.devices.iter().enumerate() {
            match device {
                Device::Resistor { a, b, ohms, .. } => {
                    stamp_conductance(system, *a, *b, 1.0 / ohms);
                }
                Device::CurrentSource { a, b, amps, .. } => {
                    stamp_current_source(system, *a, *b, *amps);
                }
                Device::VoltageSource { a, b, volts, .. } => {
                    let slot = slots[index].expect("voltage source branch slot");
                    stamp_branch_kvl(system, *a, *b, slot);
                    system.stamp_b(slot, *volts);
                }
                Device::Diode { a, b, is, vt, .. } => {
                    let guess = diode_guess.get(index).copied().unwrap_or(0.0);
                    stamp_diode_tangent(system, *a, *b, *is, *vt, guess);
                }
                Device::Capacitor { .. } => {
                    // Open circuit for DC.
                }
                Device::Inductor { a, b, .. } => {
                    // An ideal inductor is a zero-volt branch in DC analysis.
                    let slot = slots[index].expect("inductor branch slot");
                    stamp_branch_kvl(system, *a, *b, slot);
                }
            }
        }
        Ok(())
    }

    fn dc_result(
        &self,
        solution: Vec<f64>,
        slots: &[Option<usize>],
    ) -> Result<DcResult, SolveError> {
        let node_voltages = unpack_node_voltages(&solution, self.num_nodes);
        let mut currents = Vec::with_capacity(self.devices.len());
        for (index, device) in self.devices.iter().enumerate() {
            let (a, b) = terminals(device);
            let voltage = node_voltages[a] - node_voltages[b];
            let current = match device {
                Device::Resistor { ohms, .. } => voltage / ohms,
                Device::VoltageSource { .. } | Device::Inductor { .. } => {
                    solution[slots[index].expect("branch current slot")]
                }
                Device::CurrentSource { amps, .. } => *amps,
                Device::Diode { is, vt, .. } => diode_current(*is, *vt, voltage),
                Device::Capacitor { .. } => 0.0,
            };
            if !current.is_finite() {
                return Err(SolveError::BadDevice(format!(
                    "calculated non-finite current for device {}",
                    device_name(device)
                )));
            }
            currents.push(current);
        }
        Ok(DcResult {
            node_voltages,
            device_currents: currents.clone(),
            branch_currents: currents,
        })
    }

    pub(crate) fn transient_device_currents(
        &self,
        solution: &[f64],
        previous_capacitor_voltages: &[f64],
        step: f64,
        slots: &[Option<usize>],
    ) -> Vec<f64> {
        self.devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let (a, b) = terminals(device);
                let voltage = node_voltage(solution, a) - node_voltage(solution, b);
                match device {
                    Device::Resistor { ohms, .. } => voltage / ohms,
                    Device::VoltageSource { .. } | Device::Inductor { .. } => {
                        solution[slots[index].expect("branch current slot")]
                    }
                    Device::CurrentSource { amps, .. } => *amps,
                    Device::Diode { is, vt, .. } => diode_current(*is, *vt, voltage),
                    Device::Capacitor { farads, .. } => {
                        let previous = previous_capacitor_voltages[index];
                        farads / step * (voltage - previous)
                    }
                }
            })
            .collect()
    }
}

pub(crate) fn stamp_conductance(system: &mut System, a: NodeId, b: NodeId, conductance: f64) {
    if a != 0 {
        system.stamp_g(a - 1, a - 1, conductance);
    }
    if b != 0 {
        system.stamp_g(b - 1, b - 1, conductance);
    }
    if a != 0 && b != 0 {
        system.stamp_g(a - 1, b - 1, -conductance);
        system.stamp_g(b - 1, a - 1, -conductance);
    }
}

/// Positive current source from a to b. The sign follows `G * v = -I_source`.
pub(crate) fn stamp_current_source(system: &mut System, a: NodeId, b: NodeId, amps: f64) {
    if a != 0 {
        system.stamp_b(a - 1, -amps);
    }
    if b != 0 {
        system.stamp_b(b - 1, amps);
    }
}

/// KVL row and matching KCL column for a branch current oriented a -> b.
pub(crate) fn stamp_branch_kvl(system: &mut System, a: NodeId, b: NodeId, slot: usize) {
    if a != 0 {
        system.stamp_g(a - 1, slot, 1.0);
        system.stamp_g(slot, a - 1, 1.0);
    }
    if b != 0 {
        system.stamp_g(b - 1, slot, -1.0);
        system.stamp_g(slot, b - 1, -1.0);
    }
}

fn node_voltage(solution: &[f64], node: NodeId) -> f64 {
    if node == 0 {
        0.0
    } else {
        solution[node - 1]
    }
}

fn unpack_node_voltages(solution: &[f64], num_nodes: usize) -> Vec<f64> {
    let mut voltages = vec![0.0; num_nodes + 1];
    if num_nodes > 0 {
        voltages[1..=num_nodes].copy_from_slice(&solution[..num_nodes]);
    }
    voltages
}

fn terminals(device: &Device) -> (NodeId, NodeId) {
    match device {
        Device::Resistor { a, b, .. }
        | Device::VoltageSource { a, b, .. }
        | Device::CurrentSource { a, b, .. }
        | Device::Diode { a, b, .. }
        | Device::Capacitor { a, b, .. }
        | Device::Inductor { a, b, .. } => (*a, *b),
    }
}

pub(crate) fn device_name(device: &Device) -> &str {
    match device {
        Device::Resistor { name, .. }
        | Device::VoltageSource { name, .. }
        | Device::CurrentSource { name, .. }
        | Device::Diode { name, .. }
        | Device::Capacitor { name, .. }
        | Device::Inductor { name, .. } => name,
    }
}

fn positive_finite(value: f64) -> bool {
    value.is_finite() && value > 0.0
}
