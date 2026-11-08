//! Transient analysis using backward-Euler companion models.
//!
//! Capacitor: i[n] = (C / h) * (v[n] - v[n-1]).
//! Inductor:   v[n] = (L / h) * (i[n] - i[n-1]).
//! The final interval is shortened when `t_stop` is not an integer multiple of
//! `t_step`, so the last sample lands exactly on the requested stop time.

use crate::mna::device_name;
use crate::mna::{
    diode_current, next_diode_voltage_guess, stamp_branch_kvl, stamp_conductance,
    stamp_current_source, stamp_diode_tangent, Circuit, Device, SolveError, System,
};

const MAX_TRANSIENT_STEPS: usize = 1_000_000;
const NEWTON_TOLERANCE_V: f64 = 1e-9;
const MAX_DIODE_ITERATIONS: usize = 200;

#[derive(Debug, Clone, Copy)]
pub struct TransientConfig {
    pub t_stop: f64,
    pub t_step: f64,
}

/// Optional storage-state initial conditions indexed exactly like `Circuit::devices`.
/// Use `None` for devices that are not capacitors or inductors.
#[derive(Debug, Clone, Default)]
pub struct TransientInitialConditions {
    pub capacitor_voltages: Vec<Option<f64>>,
    pub inductor_currents: Vec<Option<f64>>,
}

pub struct TransientResult {
    pub times: Vec<f64>,
    /// `node_voltages[step][node]`; index 0 is ground.
    pub node_voltages: Vec<Vec<f64>>,
    /// Signed device currents in each device's a -> b reference direction.
    pub device_currents: Vec<Vec<f64>>,
}

impl Circuit {
    pub fn transient(&self, config: &TransientConfig) -> Result<TransientResult, SolveError> {
        self.transient_with_initial_conditions(config, &TransientInitialConditions::default())
    }

    pub fn transient_with_initial_conditions(
        &self,
        config: &TransientConfig,
        initial: &TransientInitialConditions,
    ) -> Result<TransientResult, SolveError> {
        if !config.t_stop.is_finite()
            || !config.t_step.is_finite()
            || config.t_stop <= 0.0
            || config.t_step <= 0.0
        {
            return Err(SolveError::BadDevice(
                "transient requires finite t_stop > 0 and t_step > 0".into(),
            ));
        }
        self.validate()?;

        let estimated_steps = (config.t_stop / config.t_step).ceil();
        if !estimated_steps.is_finite() || estimated_steps > MAX_TRANSIENT_STEPS as f64 {
            return Err(SolveError::BadDevice(format!(
                "transient requires more than {MAX_TRANSIENT_STEPS} steps; increase the time step"
            )));
        }
        let unknowns = self.num_nodes + self.extra_unknowns();
        if unknowns == 0 {
            return Err(SolveError::BadDevice("circuit has no unknowns".into()));
        }

        if (!initial.capacitor_voltages.is_empty()
            && initial.capacitor_voltages.len() != self.devices.len())
            || (!initial.inductor_currents.is_empty()
                && initial.inductor_currents.len() != self.devices.len())
        {
            return Err(SolveError::BadDevice(
                "initial-condition arrays must be empty or match the circuit device count".into(),
            ));
        }
        for value in initial
            .capacitor_voltages
            .iter()
            .chain(initial.inductor_currents.iter())
            .flatten()
        {
            if !value.is_finite() {
                return Err(SolveError::BadDevice(
                    "storage initial conditions must be finite".into(),
                ));
            }
        }

        let slots = self.branch_slots();
        let has_diodes = self
            .devices
            .iter()
            .any(|device| matches!(device, Device::Diode { .. }));
        let capacity = estimated_steps as usize + 1;
        let mut times = Vec::with_capacity(capacity);
        let mut node_voltages = Vec::with_capacity(capacity);
        let mut device_currents = Vec::with_capacity(capacity);

        // Default storage states are zero; caller-provided initial conditions overwrite them.
        let mut capacitor_voltage_previous = vec![0.0; self.devices.len()];
        let mut inductor_current_previous = vec![0.0; self.devices.len()];
        if !initial.capacitor_voltages.is_empty() {
            for (index, value) in initial.capacitor_voltages.iter().enumerate() {
                if let Some(value) = value {
                    if !matches!(self.devices[index], Device::Capacitor { .. }) {
                        return Err(SolveError::BadDevice(format!(
                            "capacitor initial voltage supplied for non-capacitor device {index}"
                        )));
                    }
                    capacitor_voltage_previous[index] = *value;
                }
            }
        }
        if !initial.inductor_currents.is_empty() {
            for (index, value) in initial.inductor_currents.iter().enumerate() {
                if let Some(value) = value {
                    if !matches!(self.devices[index], Device::Inductor { .. }) {
                        return Err(SolveError::BadDevice(format!(
                            "inductor initial current supplied for non-inductor device {index}"
                        )));
                    }
                    inductor_current_previous[index] = *value;
                }
            }
        }
        // Solve the t=0 algebraic constraints explicitly: capacitor voltages
        // are fixed to their initial values and inductor currents are fixed to
        // their initial values. This makes the first plotted sample physically
        // consistent instead of always reporting an all-zero node vector.
        let (initial_solution, capacitor_slots, mut diode_voltage_guess) = self
            .solve_initial_state(
                &capacitor_voltage_previous,
                &inductor_current_previous,
                &slots,
            )?;
        let initial_voltage_row = unpack_initial_node_voltages(&initial_solution, self.num_nodes);
        let initial_current_row =
            self.initial_device_currents(&initial_solution, &slots, &capacitor_slots)?;
        times.push(0.0);
        node_voltages.push(initial_voltage_row);
        device_currents.push(initial_current_row);

        let mut elapsed = 0.0;
        while elapsed < config.t_stop {
            let step = (config.t_stop - elapsed).min(config.t_step);
            if step <= 0.0 || !step.is_finite() {
                return Err(SolveError::BadDevice(
                    "transient time-step underflow".into(),
                ));
            }

            let mut accepted_solution = vec![0.0; unknowns];
            let mut converged = !has_diodes;
            let iterations = if has_diodes { MAX_DIODE_ITERATIONS } else { 1 };

            for _ in 0..iterations {
                let mut system = System::new(unknowns);
                self.stamp_transient(
                    &mut system,
                    step,
                    &capacitor_voltage_previous,
                    &inductor_current_previous,
                    &diode_voltage_guess,
                    &slots,
                )?;
                accepted_solution = system.solve()?;

                if !has_diodes {
                    converged = true;
                    break;
                }

                let mut largest_error = 0.0_f64;
                for (index, device) in self.devices.iter().enumerate() {
                    if let Device::Diode { a, b, .. } = device {
                        let solved_voltage =
                            voltage_at(&accepted_solution, *a) - voltage_at(&accepted_solution, *b);
                        let delta = solved_voltage - diode_voltage_guess[index];
                        largest_error = largest_error.max(delta.abs());
                        diode_voltage_guess[index] =
                            next_diode_voltage_guess(diode_voltage_guess[index], solved_voltage);
                    }
                }
                if largest_error < NEWTON_TOLERANCE_V {
                    converged = true;
                    break;
                }
            }

            if !converged {
                return Err(SolveError::NoConvergence(MAX_DIODE_ITERATIONS));
            }

            // Calculate currents against the previous storage state before committing it.
            let current_row = self.transient_device_currents(
                &accepted_solution,
                &capacitor_voltage_previous,
                step,
                &slots,
            );
            if current_row.iter().any(|current| !current.is_finite()) {
                return Err(SolveError::BadDevice(
                    "transient calculation produced a non-finite device current".into(),
                ));
            }

            // Commit storage state only after a converged step.
            for (index, device) in self.devices.iter().enumerate() {
                match device {
                    Device::Capacitor { a, b, .. } => {
                        capacitor_voltage_previous[index] =
                            voltage_at(&accepted_solution, *a) - voltage_at(&accepted_solution, *b);
                    }
                    Device::Inductor { .. } => {
                        inductor_current_previous[index] =
                            accepted_solution[slots[index].expect("inductor branch slot")];
                    }
                    _ => {}
                }
            }

            let next_elapsed = elapsed + step;
            if next_elapsed <= elapsed {
                return Err(SolveError::BadDevice(
                    "transient time step is too small to advance the simulation clock".into(),
                ));
            }
            elapsed = next_elapsed;
            // Avoid a floating-point loop at the endpoint and retain the exact
            // user-requested stop time in the result axis.
            if (config.t_stop - elapsed).abs() <= config.t_stop.abs() * 1e-12 {
                elapsed = config.t_stop;
            }
            let mut voltage_row = vec![0.0; self.num_nodes + 1];
            if self.num_nodes > 0 {
                voltage_row[1..=self.num_nodes]
                    .copy_from_slice(&accepted_solution[..self.num_nodes]);
            }
            times.push(elapsed);
            node_voltages.push(voltage_row);
            device_currents.push(current_row);
        }

        Ok(TransientResult {
            times,
            node_voltages,
            device_currents,
        })
    }

    fn solve_initial_state(
        &self,
        capacitor_voltage_previous: &[f64],
        inductor_current_previous: &[f64],
        slots: &[Option<usize>],
    ) -> Result<(Vec<f64>, Vec<Option<usize>>, Vec<f64>), SolveError> {
        let base_unknowns = self.num_nodes + self.extra_unknowns();
        let mut next_slot = base_unknowns;
        let capacitor_slots: Vec<Option<usize>> = self
            .devices
            .iter()
            .map(|device| {
                if matches!(device, Device::Capacitor { .. }) {
                    let slot = next_slot;
                    next_slot += 1;
                    Some(slot)
                } else {
                    None
                }
            })
            .collect();
        let unknowns = next_slot;
        let has_diodes = self
            .devices
            .iter()
            .any(|device| matches!(device, Device::Diode { .. }));
        let mut diode_voltage_guess = vec![0.0; self.devices.len()];
        let iterations = if has_diodes { MAX_DIODE_ITERATIONS } else { 1 };

        for _ in 0..iterations {
            let mut system = System::new(unknowns);
            for (index, device) in self.devices.iter().enumerate() {
                match device {
                    Device::Resistor { a, b, ohms, .. } => {
                        stamp_conductance(&mut system, *a, *b, 1.0 / ohms);
                    }
                    Device::CurrentSource { a, b, amps, .. } => {
                        stamp_current_source(&mut system, *a, *b, *amps);
                    }
                    Device::VoltageSource { a, b, volts, .. } => {
                        let slot = slots[index].expect("voltage-source branch slot");
                        stamp_branch_kvl(&mut system, *a, *b, slot);
                        system.stamp_b(slot, *volts);
                    }
                    Device::Diode { a, b, is, vt, .. } => {
                        stamp_diode_tangent(
                            &mut system,
                            *a,
                            *b,
                            *is,
                            *vt,
                            diode_voltage_guess[index],
                        );
                    }
                    Device::Capacitor { a, b, .. } => {
                        let slot = capacitor_slots[index].expect("capacitor initial-voltage slot");
                        stamp_branch_kvl(&mut system, *a, *b, slot);
                        system.stamp_b(slot, capacitor_voltage_previous[index]);
                    }
                    Device::Inductor { a, b, .. } => {
                        let slot = slots[index].expect("inductor branch-current slot");
                        stamp_branch_current_column(&mut system, *a, *b, slot);
                        // At t=0 the inductor current is its specified state;
                        // its differential voltage equation becomes active at step 1.
                        system.stamp_g(slot, slot, 1.0);
                        system.stamp_b(slot, inductor_current_previous[index]);
                    }
                }
            }

            let solution = system.solve()?;
            if !has_diodes {
                return Ok((solution, capacitor_slots, diode_voltage_guess));
            }

            let mut largest_error = 0.0_f64;
            for (index, device) in self.devices.iter().enumerate() {
                if let Device::Diode { a, b, .. } = device {
                    let solved_voltage = voltage_at(&solution, *a) - voltage_at(&solution, *b);
                    let delta = solved_voltage - diode_voltage_guess[index];
                    largest_error = largest_error.max(delta.abs());
                    diode_voltage_guess[index] =
                        next_diode_voltage_guess(diode_voltage_guess[index], solved_voltage);
                }
            }
            if largest_error < NEWTON_TOLERANCE_V {
                return Ok((solution, capacitor_slots, diode_voltage_guess));
            }
        }
        Err(SolveError::NoConvergence(MAX_DIODE_ITERATIONS))
    }

    fn initial_device_currents(
        &self,
        solution: &[f64],
        slots: &[Option<usize>],
        capacitor_slots: &[Option<usize>],
    ) -> Result<Vec<f64>, SolveError> {
        let mut currents = Vec::with_capacity(self.devices.len());
        for (index, device) in self.devices.iter().enumerate() {
            let (a, b) = device_terminals(device);
            let voltage = voltage_at(solution, a) - voltage_at(solution, b);
            let current = match device {
                Device::Resistor { ohms, .. } => voltage / ohms,
                Device::VoltageSource { .. } | Device::Inductor { .. } => {
                    solution[slots[index].expect("branch-current slot")]
                }
                Device::CurrentSource { amps, .. } => *amps,
                Device::Diode { is, vt, .. } => diode_current(*is, *vt, voltage),
                Device::Capacitor { .. } => {
                    solution[capacitor_slots[index].expect("capacitor current slot")]
                }
            };
            if !current.is_finite() {
                return Err(SolveError::BadDevice(format!(
                    "calculated non-finite initial current for device {}",
                    device_name(device)
                )));
            }
            currents.push(current);
        }
        Ok(currents)
    }

    fn stamp_transient(
        &self,
        system: &mut System,
        step: f64,
        capacitor_voltage_previous: &[f64],
        inductor_current_previous: &[f64],
        diode_voltage_guess: &[f64],
        slots: &[Option<usize>],
    ) -> Result<(), SolveError> {
        self.stamp_base(system, diode_voltage_guess, slots)?;

        for (index, device) in self.devices.iter().enumerate() {
            match device {
                Device::Capacitor { a, b, farads, .. } => {
                    let conductance = farads / step;
                    stamp_conductance(system, *a, *b, conductance);
                    // Norton companion source is -G * V_previous, directed a -> b.
                    let equivalent_current = -conductance * capacitor_voltage_previous[index];
                    stamp_current_source(system, *a, *b, equivalent_current);
                }
                Device::Inductor { henries, .. } => {
                    let branch = slots[index].expect("inductor branch slot");
                    let resistance = henries / step;
                    // V(a)-V(b) - (L/h) * i[n] = -(L/h) * i[n-1].
                    system.stamp_g(branch, branch, -resistance);
                    system.stamp_b(branch, -resistance * inductor_current_previous[index]);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn stamp_branch_current_column(system: &mut System, a: usize, b: usize, slot: usize) {
    if a != 0 {
        system.stamp_g(a - 1, slot, 1.0);
    }
    if b != 0 {
        system.stamp_g(b - 1, slot, -1.0);
    }
}

fn device_terminals(device: &Device) -> (usize, usize) {
    match device {
        Device::Resistor { a, b, .. }
        | Device::VoltageSource { a, b, .. }
        | Device::CurrentSource { a, b, .. }
        | Device::Diode { a, b, .. }
        | Device::Capacitor { a, b, .. }
        | Device::Inductor { a, b, .. } => (*a, *b),
    }
}

fn unpack_initial_node_voltages(solution: &[f64], num_nodes: usize) -> Vec<f64> {
    let mut voltages = vec![0.0; num_nodes + 1];
    if num_nodes > 0 {
        voltages[1..=num_nodes].copy_from_slice(&solution[..num_nodes]);
    }
    voltages
}

fn voltage_at(solution: &[f64], node: usize) -> f64 {
    if node == 0 {
        0.0
    } else {
        solution[node - 1]
    }
}
