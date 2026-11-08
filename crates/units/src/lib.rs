//! Physical quantities with SI dimensional checking and engineering prefixes.
//! All stored quantity values are converted to canonical SI values by `si()`.

use std::fmt;

/// Dimension vector: [L, M, T, I, K, N, J] exponents (SI base dimensions).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dimension(pub [i8; 7]);

pub const DIMENSIONLESS: Dimension = Dimension([0; 7]);
pub const VOLTAGE: Dimension = Dimension([2, 1, -3, -1, 0, 0, 0]);
pub const CURRENT: Dimension = Dimension([0, 0, 0, 1, 0, 0, 0]);
pub const RESISTANCE: Dimension = Dimension([2, 1, -3, -2, 0, 0, 0]);
pub const CAPACITANCE: Dimension = Dimension([-2, -1, 4, 2, 0, 0, 0]);
pub const INDUCTANCE: Dimension = Dimension([2, 1, -2, -2, 0, 0, 0]);
pub const FREQUENCY: Dimension = Dimension([0, 0, -1, 0, 0, 0, 0]);
pub const TIME: Dimension = Dimension([0, 0, 1, 0, 0, 0, 0]);
pub const POWER: Dimension = Dimension([2, 1, -3, 0, 0, 0, 0]);
pub const ANGLE: Dimension = DIMENSIONLESS;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Prefix {
    Pico,
    Nano,
    Micro,
    Milli,
    None,
    Kilo,
    Mega,
    Giga,
}

impl Prefix {
    pub fn factor(self) -> f64 {
        match self {
            Prefix::Pico => 1e-12,
            Prefix::Nano => 1e-9,
            Prefix::Micro => 1e-6,
            Prefix::Milli => 1e-3,
            Prefix::None => 1.0,
            Prefix::Kilo => 1e3,
            Prefix::Mega => 1e6,
            Prefix::Giga => 1e9,
        }
    }
}

/// A physical quantity: value and prefix with a compile-time-independent SI dimension.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quantity {
    pub value: f64,
    pub prefix: Prefix,
    pub dimension: Dimension,
}

impl Quantity {
    pub fn new(value: f64, prefix: Prefix, dimension: Dimension) -> Self {
        Self {
            value,
            prefix,
            dimension,
        }
    }

    /// Canonical SI base-unit value (e.g. volts, ohms, farads).
    pub fn si(self) -> f64 {
        self.value * self.prefix.factor()
    }

    pub fn add(self, other: Quantity) -> Result<Quantity, UnitError> {
        if self.dimension != other.dimension {
            return Err(UnitError::DimensionMismatch {
                lhs: self.dimension,
                rhs: other.dimension,
            });
        }
        let result = self.si() + other.si();
        if !result.is_finite() {
            return Err(UnitError::NonFinite);
        }
        Ok(Quantity::new(result, Prefix::None, self.dimension))
    }

    pub fn mul(self, other: Quantity) -> Quantity {
        let mut dimension = [0_i8; 7];
        for (index, exponent) in dimension.iter_mut().enumerate() {
            *exponent = self.dimension.0[index].saturating_add(other.dimension.0[index]);
        }
        Quantity::new(self.si() * other.si(), Prefix::None, Dimension(dimension))
    }

    pub fn div(self, other: Quantity) -> Result<Quantity, UnitError> {
        if other.si() == 0.0 {
            return Err(UnitError::DivideByZero);
        }
        let mut dimension = [0_i8; 7];
        for (index, exponent) in dimension.iter_mut().enumerate() {
            *exponent = self.dimension.0[index].saturating_sub(other.dimension.0[index]);
        }
        let result = self.si() / other.si();
        if !result.is_finite() {
            return Err(UnitError::NonFinite);
        }
        Ok(Quantity::new(result, Prefix::None, Dimension(dimension)))
    }
}

#[derive(Debug, PartialEq)]
pub enum UnitError {
    DimensionMismatch { lhs: Dimension, rhs: Dimension },
    DivideByZero,
    UnknownUnit(String),
    NonFinite,
}

impl fmt::Display for UnitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnitError::DimensionMismatch { .. } => {
                write!(formatter, "quantity dimensions do not match")
            }
            UnitError::DivideByZero => write!(formatter, "division by a zero-valued quantity"),
            UnitError::UnknownUnit(unit) => {
                write!(formatter, "unknown or invalid engineering value '{unit}'")
            }
            UnitError::NonFinite => write!(
                formatter,
                "quantity calculation produced a non-finite value"
            ),
        }
    }
}
impl std::error::Error for UnitError {}

/// Parse values such as `4.7k`, `10uF`, `5 V`, `2.2 kOhm`, or `100nF`.
/// Explicit unit symbols are checked against `expected`; bare values and
/// prefix-only suffixes use the supplied expected dimension.
pub fn parse_engineering(input: &str, expected: Dimension) -> Result<Quantity, UnitError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(UnitError::UnknownUnit(input.into()));
    }

    let split = text.find(|character: char| {
        !(character.is_ascii_digit() || matches!(character, '.' | '-' | '+' | 'e' | 'E'))
    });
    let (number_text, suffix) = match split {
        Some(index) => text.split_at(index),
        None => (text, ""),
    };
    let value = number_text
        .trim()
        .parse::<f64>()
        .map_err(|_| UnitError::UnknownUnit(input.into()))?;
    if !value.is_finite() {
        return Err(UnitError::NonFinite);
    }

    let (prefix, explicit_dimension) = parse_suffix(suffix.trim())
        .ok_or_else(|| UnitError::UnknownUnit(suffix.trim().to_string()))?;
    if let Some(actual) = explicit_dimension {
        if actual != expected {
            return Err(UnitError::DimensionMismatch {
                lhs: expected,
                rhs: actual,
            });
        }
    }
    let quantity = Quantity::new(value, prefix, expected);
    if !quantity.si().is_finite() {
        return Err(UnitError::NonFinite);
    }
    Ok(quantity)
}

fn parse_suffix(suffix: &str) -> Option<(Prefix, Option<Dimension>)> {
    let result = match suffix {
        "" => (Prefix::None, None),
        "p" => (Prefix::Pico, None),
        "n" => (Prefix::Nano, None),
        "u" | "µ" => (Prefix::Micro, None),
        "m" => (Prefix::Milli, None),
        "k" | "K" => (Prefix::Kilo, None),
        "M" | "meg" => (Prefix::Mega, None),
        "G" => (Prefix::Giga, None),
        "V" => (Prefix::None, Some(VOLTAGE)),
        "mV" => (Prefix::Milli, Some(VOLTAGE)),
        "uV" | "µV" => (Prefix::Micro, Some(VOLTAGE)),
        "kV" => (Prefix::Kilo, Some(VOLTAGE)),
        "A" => (Prefix::None, Some(CURRENT)),
        "mA" => (Prefix::Milli, Some(CURRENT)),
        "uA" | "µA" => (Prefix::Micro, Some(CURRENT)),
        "nA" => (Prefix::Nano, Some(CURRENT)),
        "pA" => (Prefix::Pico, Some(CURRENT)),
        "kA" => (Prefix::Kilo, Some(CURRENT)),
        "ohm" | "Ohm" | "Ω" | "R" => (Prefix::None, Some(RESISTANCE)),
        "mOhm" | "mΩ" => (Prefix::Milli, Some(RESISTANCE)),
        "kOhm" | "kohm" | "kΩ" => (Prefix::Kilo, Some(RESISTANCE)),
        "MOhm" | "Mohm" | "MΩ" => (Prefix::Mega, Some(RESISTANCE)),
        "F" => (Prefix::None, Some(CAPACITANCE)),
        "mF" => (Prefix::Milli, Some(CAPACITANCE)),
        "uF" | "µF" => (Prefix::Micro, Some(CAPACITANCE)),
        "nF" => (Prefix::Nano, Some(CAPACITANCE)),
        "pF" => (Prefix::Pico, Some(CAPACITANCE)),
        "H" => (Prefix::None, Some(INDUCTANCE)),
        "mH" => (Prefix::Milli, Some(INDUCTANCE)),
        "uH" | "µH" => (Prefix::Micro, Some(INDUCTANCE)),
        "nH" => (Prefix::Nano, Some(INDUCTANCE)),
        "s" => (Prefix::None, Some(TIME)),
        "ms" => (Prefix::Milli, Some(TIME)),
        "us" | "µs" => (Prefix::Micro, Some(TIME)),
        "ns" => (Prefix::Nano, Some(TIME)),
        "Hz" => (Prefix::None, Some(FREQUENCY)),
        "kHz" => (Prefix::Kilo, Some(FREQUENCY)),
        "MHz" => (Prefix::Mega, Some(FREQUENCY)),
        "W" => (Prefix::None, Some(POWER)),
        "mW" => (Prefix::Milli, Some(POWER)),
        "kW" => (Prefix::Kilo, Some(POWER)),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engineering_parse_and_units() {
        assert!((parse_engineering("4.7k", RESISTANCE).unwrap().si() - 4700.0).abs() < 1e-9);
        assert!((parse_engineering("100nF", CAPACITANCE).unwrap().si() - 100e-9).abs() < 1e-15);
        assert!((parse_engineering("2.2 kOhm", RESISTANCE).unwrap().si() - 2200.0).abs() < 1e-9);
        assert!((parse_engineering("12mV", VOLTAGE).unwrap().si() - 0.012).abs() < 1e-12);
        assert!((parse_engineering("2.52nA", CURRENT).unwrap().si() - 2.52e-9).abs() < 1e-18);
    }

    #[test]
    fn dimension_rules_and_ohms_law() {
        let voltage = Quantity::new(10.0, Prefix::None, VOLTAGE);
        let current = Quantity::new(0.5, Prefix::Milli, CURRENT);
        let resistance = voltage.div(current).unwrap();
        assert_eq!(resistance.dimension, RESISTANCE);
        assert!((resistance.si() - 20_000.0).abs() < 1e-6);
        assert!(voltage.add(current).is_err());
    }

    #[test]
    fn power_law_and_invalid_inputs() {
        let current = Quantity::new(2.0, Prefix::None, CURRENT);
        let resistance = Quantity::new(5.0, Prefix::None, RESISTANCE);
        let power = current.mul(current).mul(resistance);
        assert_eq!(power.dimension, POWER);
        assert!((power.si() - 20.0).abs() < 1e-12);
        assert!(parse_engineering("1kOhm", VOLTAGE).is_err());
        assert!(parse_engineering("NaN", VOLTAGE).is_err());
    }
}
