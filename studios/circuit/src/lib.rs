//! Circuit Studio's schematic-to-solver boundary.
//!
//! Wire connectivity is represented by terminal references. A disjoint-set
//! pass resolves nets to deterministic solver node numbers before a circuit
//! is sent to the MNA engine.

use eee_numerical::{Circuit, Device, SolveError};
use std::collections::{HashMap, HashSet};

pub const DEFAULT_DIODE_SATURATION_CURRENT_A: f64 = 2.52e-9;
pub const DEFAULT_DIODE_THERMAL_VOLTAGE_V: f64 = 0.02585;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Terminal {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PinRef {
    pub component_id: String,
    pub terminal: Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wire {
    pub from: PinRef,
    pub to: PinRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Resistor,
    Capacitor,
    Inductor,
    VoltageSource,
    CurrentSource,
    Diode,
    Ground,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchematicElement {
    pub id: String,
    pub kind: ElementKind,
    pub label: String,
    /// SI value: ohms, farads, henries, volts or amps. For a diode this is Is.
    pub value: f64,
    /// Schematic-space coordinates, in logical pixels.
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CircuitDocument {
    pub elements: Vec<SchematicElement>,
    pub wires: Vec<Wire>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CircuitBuild {
    pub circuit: Circuit,
    /// Stable labels for display: `Node 1`, `Node 2`, etc.; index 0 is Ground.
    pub node_labels: Vec<String>,
}

impl CircuitDocument {
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut ground_count = 0;
        for element in &self.elements {
            if element.id.trim().is_empty() {
                return Err("component id cannot be empty".into());
            }
            if !ids.insert(element.id.clone()) {
                return Err(format!("duplicate component id '{}'", element.id));
            }
            if !element.x.is_finite() || !element.y.is_finite() {
                return Err(format!(
                    "component '{}' has a non-finite schematic position",
                    element.id
                ));
            }
            if element.label.trim().is_empty() {
                return Err(format!("component '{}' needs a label", element.id));
            }
            if element.kind == ElementKind::Ground {
                ground_count += 1;
            } else if !element.value.is_finite() {
                return Err(format!("component '{}' has a non-finite value", element.id));
            }
        }
        if ground_count == 0 {
            return Err("place at least one ground symbol before simulating".into());
        }
        for wire in &self.wires {
            self.validate_pin(&wire.from)?;
            self.validate_pin(&wire.to)?;
            if wire.from == wire.to {
                return Err("wire endpoints must be different terminals".into());
            }
        }
        Ok(())
    }

    pub fn build_solver_circuit(&self) -> Result<CircuitBuild, String> {
        self.validate()?;
        let mut terminal_indices: HashMap<PinRef, usize> = HashMap::new();
        let mut ordered_terminals: Vec<(PinRef, usize)> = Vec::new();
        let mut next_index = 0;
        for element in &self.elements {
            let a = PinRef {
                component_id: element.id.clone(),
                terminal: Terminal::A,
            };
            terminal_indices.insert(a.clone(), next_index);
            ordered_terminals.push((a, next_index));
            next_index += 1;
            if element.kind != ElementKind::Ground {
                let b = PinRef {
                    component_id: element.id.clone(),
                    terminal: Terminal::B,
                };
                terminal_indices.insert(b.clone(), next_index);
                ordered_terminals.push((b, next_index));
                next_index += 1;
            }
        }

        let mut sets = DisjointSet::new(next_index);
        let ground_indices: Vec<usize> = self
            .elements
            .iter()
            .filter(|element| element.kind == ElementKind::Ground)
            .filter_map(|element| {
                terminal_indices
                    .get(&PinRef {
                        component_id: element.id.clone(),
                        terminal: Terminal::A,
                    })
                    .copied()
            })
            .collect();
        let ground_root_index = *ground_indices.first().ok_or("ground terminal not found")?;
        for index in ground_indices.iter().skip(1) {
            sets.union(ground_root_index, *index);
        }
        for wire in &self.wires {
            let from = *terminal_indices
                .get(&wire.from)
                .ok_or("wire start terminal not found")?;
            let to = *terminal_indices
                .get(&wire.to)
                .ok_or("wire end terminal not found")?;
            sets.union(from, to);
        }

        let ground_root = sets.find(ground_root_index);
        let mut root_to_node: HashMap<usize, usize> = HashMap::new();
        let mut next_node = 1;
        let mut terminal_nodes: HashMap<PinRef, usize> = HashMap::new();
        for (pin, index) in &ordered_terminals {
            let root = sets.find(*index);
            let node = if root == ground_root {
                0
            } else {
                *root_to_node.entry(root).or_insert_with(|| {
                    let assigned = next_node;
                    next_node += 1;
                    assigned
                })
            };
            terminal_nodes.insert(pin.clone(), node);
        }

        let mut devices = Vec::new();
        for element in &self.elements {
            if element.kind == ElementKind::Ground {
                continue;
            }
            let a = terminal_nodes[&PinRef {
                component_id: element.id.clone(),
                terminal: Terminal::A,
            }];
            let b = terminal_nodes[&PinRef {
                component_id: element.id.clone(),
                terminal: Terminal::B,
            }];
            let name = element.label.clone();
            let device = match element.kind {
                ElementKind::Resistor => Device::Resistor {
                    name,
                    a,
                    b,
                    ohms: element.value,
                },
                ElementKind::Capacitor => Device::Capacitor {
                    name,
                    a,
                    b,
                    farads: element.value,
                },
                ElementKind::Inductor => Device::Inductor {
                    name,
                    a,
                    b,
                    henries: element.value,
                },
                ElementKind::VoltageSource => Device::VoltageSource {
                    name,
                    a,
                    b,
                    volts: element.value,
                },
                ElementKind::CurrentSource => Device::CurrentSource {
                    name,
                    a,
                    b,
                    amps: element.value,
                },
                ElementKind::Diode => Device::Diode {
                    name,
                    a,
                    b,
                    is: element.value,
                    vt: DEFAULT_DIODE_THERMAL_VOLTAGE_V,
                },
                ElementKind::Ground => unreachable!("ground elements were skipped"),
            };
            devices.push(device);
        }

        let circuit = Circuit {
            num_nodes: next_node - 1,
            devices,
        };
        circuit
            .validate()
            .map_err(|error: SolveError| error.to_string())?;
        let mut node_labels = vec!["Ground".to_string(); next_node];
        for (node, label) in node_labels.iter_mut().enumerate().skip(1) {
            *label = format!("Node {node}");
        }
        Ok(CircuitBuild {
            circuit,
            node_labels,
        })
    }

    fn validate_pin(&self, pin: &PinRef) -> Result<(), String> {
        let element = self
            .elements
            .iter()
            .find(|element| element.id == pin.component_id)
            .ok_or_else(|| format!("wire references missing component '{}'", pin.component_id))?;
        if element.kind == ElementKind::Ground && pin.terminal == Terminal::B {
            return Err(format!("ground '{}' has only terminal A", element.id));
        }
        Ok(())
    }
}

struct DisjointSet {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl DisjointSet {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, item: usize) -> usize {
        if self.parent[item] != item {
            let root = self.find(self.parent[item]);
            self.parent[item] = root;
        }
        self.parent[item]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left_root = self.find(left);
        let mut right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        if self.rank[left_root] < self.rank[right_root] {
            std::mem::swap(&mut left_root, &mut right_root);
        }
        self.parent[right_root] = left_root;
        if self.rank[left_root] == self.rank[right_root] {
            self.rank[left_root] = self.rank[left_root].saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: &str, kind: ElementKind, value: f64, x: f64) -> SchematicElement {
        SchematicElement {
            id: id.into(),
            kind,
            label: id.into(),
            value,
            x,
            y: 100.0,
        }
    }

    fn pin(id: &str, terminal: Terminal) -> PinRef {
        PinRef {
            component_id: id.into(),
            terminal,
        }
    }

    #[test]
    fn wire_graph_builds_voltage_divider_nodes() {
        let document = CircuitDocument {
            elements: vec![
                element("V1", ElementKind::VoltageSource, 10.0, 80.0),
                element("R1", ElementKind::Resistor, 1000.0, 220.0),
                element("R2", ElementKind::Resistor, 1000.0, 360.0),
                element("GND", ElementKind::Ground, 0.0, 500.0),
            ],
            wires: vec![
                Wire {
                    from: pin("V1", Terminal::A),
                    to: pin("R1", Terminal::A),
                },
                Wire {
                    from: pin("R1", Terminal::B),
                    to: pin("R2", Terminal::A),
                },
                Wire {
                    from: pin("V1", Terminal::B),
                    to: pin("GND", Terminal::A),
                },
                Wire {
                    from: pin("R2", Terminal::B),
                    to: pin("GND", Terminal::A),
                },
            ],
        };
        let build = document.build_solver_circuit().unwrap();
        let result = build.circuit.dc().unwrap();
        assert!((result.node_voltages[1] - 10.0).abs() < 1e-9);
        assert!((result.node_voltages[2] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn unconnected_schematic_node_is_not_silently_grounded() {
        let document = CircuitDocument {
            elements: vec![
                element("V1", ElementKind::VoltageSource, 5.0, 100.0),
                element("R1", ElementKind::Resistor, 1000.0, 250.0),
                element("GND", ElementKind::Ground, 0.0, 400.0),
            ],
            wires: vec![Wire {
                from: pin("V1", Terminal::B),
                to: pin("GND", Terminal::A),
            }],
        };
        let build = document.build_solver_circuit().unwrap();
        assert!(build.circuit.dc().is_err());
    }

    #[test]
    fn duplicate_ids_and_invalid_terminals_are_rejected() {
        let duplicate = CircuitDocument {
            elements: vec![
                element("R1", ElementKind::Resistor, 1000.0, 0.0),
                element("R1", ElementKind::Resistor, 2000.0, 0.0),
                element("GND", ElementKind::Ground, 0.0, 0.0),
            ],
            wires: vec![],
        };
        assert!(duplicate.validate().is_err());

        let invalid_pin = CircuitDocument {
            elements: vec![element("GND", ElementKind::Ground, 0.0, 0.0)],
            wires: vec![Wire {
                from: pin("GND", Terminal::B),
                to: pin("GND", Terminal::A),
            }],
        };
        assert!(invalid_pin.validate().is_err());
    }
}
