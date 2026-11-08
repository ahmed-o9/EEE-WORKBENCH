//! Unified engineering model graph.
//!
//! Every studio reads/writes the same graph: components, connections,
//! parameters (SI-validated via eee-units), geometry refs and results refs.
//! This is what lets a motor-drive project share state across Circuit,
//! Control and Machines studios without duplicating the model.

use eee_units::{Quantity, UnitError};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentId(pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub quantity: Quantity,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComponentKind {
    // Circuit Studio
    Resistor,
    Capacitor,
    Inductor,
    VoltageSource,
    CurrentSource,
    Diode,
    // Machines Studio (planned: dynamic models connect to the same graph)
    DcMachine,
    InductionMachine,
    SynchronousMachine,
    // Control Studio
    TransferFunction,
    PidController,
    // Electromagnetics
    ConductingDomain,
    MagneticDomain,
}

#[derive(Debug, Clone)]
pub struct Component {
    pub id: ComponentId,
    pub kind: ComponentKind,
    pub name: String,
    pub params: Vec<Parameter>,
    /// Free-form studio payload (e.g. schematic position), kept opaque here.
    pub studio: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Connection {
    pub from: ComponentId,
    pub from_port: String,
    pub to: ComponentId,
    pub to_port: String,
}

#[derive(Debug, Default)]
pub struct ModelGraph {
    pub components: Vec<Component>,
    pub connections: Vec<Connection>,
}

impl ModelGraph {
    pub fn add(&mut self, c: Component) {
        self.components.push(c);
    }
    pub fn connect(&mut self, from: ComponentId, from_port: &str, to: ComponentId, to_port: &str) {
        self.connections.push(Connection {
            from,
            from_port: from_port.into(),
            to,
            to_port: to_port.into(),
        });
    }
    /// Fetch a parameter, dimension-checked by the caller's expected unit.
    pub fn param(&self, id: &ComponentId, name: &str) -> Option<&Quantity> {
        self.components
            .iter()
            .find(|c| &c.id == id)
            .and_then(|c| c.params.iter().find(|p| p.name == name))
            .map(|p| &p.quantity)
    }
    /// Cross-check: every connection references existing components.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for component in &self.components {
            if component.id.0.trim().is_empty() {
                return Err("component id cannot be empty".into());
            }
            if !ids.insert(component.id.clone()) {
                return Err(format!("duplicate component id {:?}", component.id));
            }
            for parameter in &component.params {
                if !parameter.quantity.si().is_finite() {
                    return Err(format!(
                        "component {:?} has a non-finite parameter '{}'",
                        component.id, parameter.name
                    ));
                }
            }
        }
        for conn in &self.connections {
            if conn.from_port.trim().is_empty() || conn.to_port.trim().is_empty() {
                return Err("connection port names cannot be empty".into());
            }
            if !self.components.iter().any(|c| c.id == conn.from) {
                return Err(format!("connection from missing component {:?}", conn.from));
            }
            if !self.components.iter().any(|c| c.id == conn.to) {
                return Err(format!("connection to missing component {:?}", conn.to));
            }
        }
        Ok(())
    }
}

pub fn checked_param(
    g: &ModelGraph,
    id: &ComponentId,
    name: &str,
    expected: eee_units::Dimension,
) -> Result<f64, UnitError> {
    match g.param(id, name) {
        Some(quantity) if quantity.dimension == expected => {
            let value = quantity.si();
            if value.is_finite() {
                Ok(value)
            } else {
                Err(UnitError::NonFinite)
            }
        }
        Some(quantity) => Err(UnitError::DimensionMismatch {
            lhs: expected,
            rhs: quantity.dimension,
        }),
        None => Err(UnitError::UnknownUnit(name.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eee_units::{Prefix, RESISTANCE};

    fn resistor(id: &str) -> Component {
        Component {
            id: ComponentId(id.into()),
            kind: ComponentKind::Resistor,
            name: id.into(),
            params: vec![Parameter {
                name: "resistance".into(),
                quantity: Quantity::new(1000.0, Prefix::None, RESISTANCE),
            }],
            studio: HashMap::new(),
        }
    }

    #[test]
    fn graph_validates_connections_and_parameters() {
        let mut graph = ModelGraph::default();
        graph.add(resistor("R1"));
        graph.add(resistor("R2"));
        graph.connect(ComponentId("R1".into()), "b", ComponentId("R2".into()), "a");
        assert!(graph.validate().is_ok());
        assert_eq!(
            checked_param(&graph, &ComponentId("R1".into()), "resistance", RESISTANCE).unwrap(),
            1000.0
        );
    }

    #[test]
    fn graph_rejects_duplicate_ids_and_missing_endpoints() {
        let mut graph = ModelGraph::default();
        graph.add(resistor("R1"));
        graph.add(resistor("R1"));
        assert!(graph.validate().unwrap_err().contains("duplicate"));
        let mut graph = ModelGraph::default();
        graph.add(resistor("R1"));
        graph.connect(ComponentId("R1".into()), "a", ComponentId("R2".into()), "b");
        assert!(graph.validate().is_err());
    }
}
