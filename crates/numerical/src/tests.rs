//! Analytical and physical validation tests for MNA and transient kernels.

use crate::linalg::LinearSolver;
use crate::linalg::{DenseLu, LuError};

use crate::mna::{Circuit, Device, SolveError};
use crate::transient::{TransientConfig, TransientInitialConditions};

fn resistor(name: &str, a: usize, b: usize, ohms: f64) -> Device {
    Device::Resistor {
        name: name.into(),
        a,
        b,
        ohms,
    }
}
fn vsource(name: &str, a: usize, b: usize, volts: f64) -> Device {
    Device::VoltageSource {
        name: name.into(),
        a,
        b,
        volts,
    }
}
fn capacitor(name: &str, a: usize, b: usize, farads: f64) -> Device {
    Device::Capacitor {
        name: name.into(),
        a,
        b,
        farads,
    }
}
fn inductor(name: &str, a: usize, b: usize, henries: f64) -> Device {
    Device::Inductor {
        name: name.into(),
        a,
        b,
        henries,
    }
}

fn assert_kcl(circuit: &Circuit, currents: &[f64], tolerance: f64) {
    let mut residual = vec![0.0; circuit.num_nodes + 1];
    for (device, current) in circuit.devices.iter().zip(currents.iter()) {
        let (a, b) = match device {
            Device::Resistor { a, b, .. }
            | Device::VoltageSource { a, b, .. }
            | Device::CurrentSource { a, b, .. }
            | Device::Diode { a, b, .. }
            | Device::Capacitor { a, b, .. }
            | Device::Inductor { a, b, .. } => (*a, *b),
        };
        residual[a] += current;
        residual[b] -= current;
    }
    for (node, error) in residual.iter().enumerate().skip(1) {
        assert!(
            error.abs() <= tolerance,
            "KCL residual at node {node}: {error}"
        );
    }
}

#[test]
fn lu_two_by_two_reference_and_residual() {
    let mut lu = DenseLu::new(2);
    lu.a[0] = vec![3.0, 1.0];
    lu.a[1] = vec![1.0, 2.0];
    lu.b = vec![9.0, 8.0];
    let x = lu.solve().unwrap();
    assert!((x[0] - 2.0).abs() < 1e-12);
    assert!((x[1] - 3.0).abs() < 1e-12);
    assert!((3.0 * x[0] + x[1] - 9.0).abs() < 1e-12);
    assert!((x[0] + 2.0 * x[1] - 8.0).abs() < 1e-12);
}

#[test]
fn lu_partial_pivoting_and_singular_detection() {
    let mut lu = DenseLu::new(2);
    lu.a[0] = vec![0.0, 1.0];
    lu.a[1] = vec![1.0, 1.0];
    lu.b = vec![1.0, 2.0];
    let x = lu.solve().unwrap();
    assert!((x[0] - 1.0).abs() < 1e-12);
    assert!((x[1] - 1.0).abs() < 1e-12);

    let mut singular = DenseLu::new(2);
    singular.a[0] = vec![1.0, 2.0];
    singular.a[1] = vec![2.0, 4.0];
    singular.b = vec![1.0, 2.0];
    assert!(matches!(singular.solve(), Err(LuError::Singular { .. })));
}

#[test]
fn lu_rejects_non_finite_coefficients() {
    let mut lu = DenseLu::new(1);
    lu.a[0][0] = f64::NAN;
    lu.b[0] = 1.0;
    assert!(matches!(lu.solve(), Err(LuError::NonFinite { .. })));
}

#[test]
fn resistor_voltage_divider_and_kcl_kvl() {
    let mut circuit = Circuit {
        num_nodes: 2,
        devices: vec![
            vsource("V1", 1, 0, 10.0),
            resistor("R1", 1, 2, 1000.0),
            resistor("R2", 2, 0, 1000.0),
        ],
    };
    let result = circuit.dc().unwrap();
    assert!((result.node_voltages[1] - 10.0).abs() < 1e-9);
    assert!((result.node_voltages[2] - 5.0).abs() < 1e-9);
    assert!((result.device_currents[0] + 0.005).abs() < 1e-9);
    assert!((result.device_currents[1] - 0.005).abs() < 1e-9);
    assert!((result.device_currents[2] - 0.005).abs() < 1e-9);
    assert_kcl(&circuit, &result.device_currents, 1e-10);
    assert!(
        (10.0 - 1000.0 * result.device_currents[1] - 1000.0 * result.device_currents[2]).abs()
            < 1e-9
    );
}

#[test]
fn parallel_resistors_and_current_source_direction() {
    let circuit = Circuit {
        num_nodes: 1,
        devices: vec![
            Device::CurrentSource {
                name: "I1".into(),
                a: 0,
                b: 1,
                amps: 0.002,
            },
            resistor("R1", 1, 0, 1000.0),
            resistor("R2", 1, 0, 1000.0),
        ],
    };
    let result = circuit.dc().unwrap();
    assert!((result.node_voltages[1] - 1.0).abs() < 1e-9);
    assert_kcl(&circuit, &result.device_currents, 1e-10);

    let reverse = Circuit {
        num_nodes: 1,
        devices: vec![
            Device::CurrentSource {
                name: "I1".into(),
                a: 1,
                b: 0,
                amps: 0.002,
            },
            resistor("R1", 1, 0, 1000.0),
        ],
    };
    assert!((reverse.dc().unwrap().node_voltages[1] + 2.0).abs() < 1e-9);
}

#[test]
fn voltage_source_polarity_and_multiple_sources() {
    let negative = Circuit {
        num_nodes: 1,
        devices: vec![vsource("V1", 1, 0, -5.0), resistor("R1", 1, 0, 1000.0)],
    };
    assert!((negative.dc().unwrap().node_voltages[1] + 5.0).abs() < 1e-9);

    let series = Circuit {
        num_nodes: 2,
        devices: vec![
            vsource("V1", 1, 0, 10.0),
            vsource("V2", 2, 1, 5.0),
            resistor("R1", 2, 0, 1000.0),
        ],
    };
    let result = series.dc().unwrap();
    assert!((result.node_voltages[2] - 15.0).abs() < 1e-9);
    assert_kcl(&series, &result.device_currents, 1e-10);
}

#[test]
fn dc_inductor_is_a_short_and_capacitor_is_open() {
    let circuit = Circuit {
        num_nodes: 2,
        devices: vec![
            vsource("V1", 1, 0, 10.0),
            resistor("R1", 1, 2, 1000.0),
            inductor("L1", 2, 0, 0.01),
            capacitor("C1", 1, 0, 1e-6),
        ],
    };
    let result = circuit.dc().unwrap();
    assert!(result.node_voltages[2].abs() < 1e-9);
    assert!((result.device_currents[2] - 0.01).abs() < 1e-9);
    assert_eq!(result.device_currents[3], 0.0);
    assert_kcl(&circuit, &result.device_currents, 1e-10);
}

#[test]
fn diode_operating_point_matches_shockley_equation() {
    let circuit = Circuit {
        num_nodes: 2,
        devices: vec![
            vsource("V1", 1, 0, 5.0),
            Device::Diode {
                name: "D1".into(),
                a: 1,
                b: 2,
                is: 2.52e-9,
                vt: 0.02585,
            },
            resistor("R1", 2, 0, 1000.0),
        ],
    };
    let result = circuit.dc().unwrap();
    let diode_voltage = result.node_voltages[1] - result.node_voltages[2];
    assert!(
        (0.35..0.40).contains(&diode_voltage),
        "diode drop was {diode_voltage}"
    );
    let expected_current = 2.52e-9 * ((diode_voltage / 0.02585).exp() - 1.0);
    assert!((result.device_currents[1] - expected_current).abs() < 1e-9);
    assert!((expected_current * 1000.0 - result.node_voltages[2]).abs() < 1e-6);
    assert_kcl(&circuit, &result.device_currents, 1e-9);
}

#[test]
fn singular_floating_and_invalid_circuits_report_errors() {
    let open = Circuit {
        num_nodes: 1,
        devices: vec![],
    };
    assert!(matches!(
        open.dc(),
        Err(SolveError::Lu(LuError::Singular { .. }))
    ));

    let floating = Circuit {
        num_nodes: 2,
        devices: vec![resistor("R1", 1, 2, 1000.0)],
    };
    assert!(matches!(
        floating.dc(),
        Err(SolveError::Lu(LuError::Singular { .. }))
    ));

    let zero_resistance = Circuit {
        num_nodes: 1,
        devices: vec![vsource("V1", 1, 0, 1.0), resistor("R0", 1, 0, 0.0)],
    };
    assert!(matches!(
        zero_resistance.dc(),
        Err(SolveError::BadDevice(_))
    ));

    let bad_node = Circuit {
        num_nodes: 1,
        devices: vec![resistor("R1", 2, 0, 1000.0)],
    };
    assert!(matches!(bad_node.dc(), Err(SolveError::BadDevice(_))));

    let bad_diode = Circuit {
        num_nodes: 1,
        devices: vec![Device::Diode {
            name: "D1".into(),
            a: 1,
            b: 0,
            is: -1.0,
            vt: 0.025,
        }],
    };
    assert!(matches!(bad_diode.dc(), Err(SolveError::BadDevice(_))));
}

#[test]
fn rc_charging_matches_exponential_and_dc_limit() {
    let circuit = Circuit {
        num_nodes: 2,
        devices: vec![
            vsource("V1", 1, 0, 10.0),
            resistor("R1", 1, 2, 1000.0),
            capacitor("C1", 2, 0, 1e-6),
        ],
    };
    let response = circuit
        .transient(&TransientConfig {
            t_stop: 0.001,
            t_step: 1e-6,
        })
        .unwrap();
    let value_at_tau = response.node_voltages[1000][2];
    let expected = 10.0 * (1.0 - (-1.0_f64).exp());
    assert!(
        (value_at_tau - expected).abs() < 0.01,
        "got {value_at_tau}, expected {expected}"
    );

    let settled = circuit
        .transient(&TransientConfig {
            t_stop: 0.01,
            t_step: 1e-5,
        })
        .unwrap();
    let final_voltage = settled.node_voltages.last().unwrap()[2];
    assert!((final_voltage - 10.0).abs() < 0.05);
}

#[test]
fn rc_discharging_matches_exponential_with_initial_condition() {
    let circuit = Circuit {
        num_nodes: 1,
        devices: vec![resistor("R1", 1, 0, 1000.0), capacitor("C1", 1, 0, 1e-6)],
    };
    let initial = TransientInitialConditions {
        capacitor_voltages: vec![None, Some(5.0)],
        inductor_currents: vec![],
    };
    let response = circuit
        .transient_with_initial_conditions(
            &TransientConfig {
                t_stop: 0.001,
                t_step: 1e-6,
            },
            &initial,
        )
        .unwrap();
    let expected = 5.0 * (-1.0_f64).exp();
    assert!((response.node_voltages[0][1] - 5.0).abs() < 1e-12);
    // The capacitor reference current is a -> b, so discharge current is negative.
    assert!((response.device_currents[0][1] + 0.005).abs() < 1e-9);
    assert_kcl(&circuit, &response.device_currents[0], 1e-12);
    assert!(response.device_currents[1000][1] < 0.0);
    assert!((response.node_voltages[1000][1] - expected).abs() < 0.01);
}

#[test]
fn series_rl_current_matches_exponential() {
    let circuit = Circuit {
        num_nodes: 2,
        devices: vec![
            vsource("V1", 1, 0, 1.0),
            resistor("R1", 1, 2, 10.0),
            inductor("L1", 2, 0, 0.1),
        ],
    };
    let response = circuit
        .transient(&TransientConfig {
            t_stop: 0.01,
            t_step: 1e-5,
        })
        .unwrap();
    let final_index = response.times.len() - 1;
    let current = response.device_currents[final_index][1];
    let expected = 0.1 * (1.0 - (-1.0_f64).exp());
    assert!(
        (current - expected).abs() < 0.002,
        "got {current}, expected {expected}"
    );
}

#[test]
fn series_rlc_step_response_matches_analytical_reference() {
    let circuit = Circuit {
        num_nodes: 3,
        devices: vec![
            vsource("V1", 1, 0, 1.0),
            resistor("R1", 1, 2, 10.0),
            inductor("L1", 2, 3, 0.1),
            capacitor("C1", 3, 0, 0.001),
        ],
    };
    let response = circuit
        .transient(&TransientConfig {
            t_stop: 0.01,
            t_step: 1e-5,
        })
        .unwrap();
    let alpha: f64 = 10.0 / (2.0 * 0.1);
    let damped_frequency = (1.0 / (0.1 * 0.001) - alpha * alpha).sqrt();
    let time: f64 = 0.01;
    let expected = 1.0
        - (-alpha * time).exp()
            * ((damped_frequency * time).cos()
                + alpha / damped_frequency * (damped_frequency * time).sin());
    let actual = response.node_voltages.last().unwrap()[3];
    assert!(
        (actual - expected).abs() < 0.003,
        "got {actual}, expected {expected}"
    );
    assert!((response.node_voltages.last().unwrap()[1] - 1.0).abs() < 0.05);
}

#[test]
fn transient_shortened_final_step_and_time_validation() {
    let circuit = Circuit {
        num_nodes: 1,
        devices: vec![vsource("V1", 1, 0, 2.0), resistor("R1", 1, 0, 1000.0)],
    };
    let response = circuit
        .transient(&TransientConfig {
            t_stop: 1.0,
            t_step: 0.3,
        })
        .unwrap();
    assert_eq!(response.times.len(), 5);
    assert_eq!(*response.times.last().unwrap(), 1.0);
    assert!((response.node_voltages.last().unwrap()[1] - 2.0).abs() < 1e-12);

    assert!(circuit
        .transient(&TransientConfig {
            t_stop: 0.0,
            t_step: 1e-3
        })
        .is_err());
    assert!(circuit
        .transient(&TransientConfig {
            t_stop: 1.0,
            t_step: 0.0
        })
        .is_err());
    assert!(circuit
        .transient(&TransientConfig {
            t_stop: 1.0,
            t_step: f64::NAN
        })
        .is_err());
    assert!(circuit
        .transient(&TransientConfig {
            t_stop: 2.0,
            t_step: 1e-7
        })
        .is_err());
}

#[test]
fn invalid_initial_condition_arrays_are_rejected() {
    let circuit = Circuit {
        num_nodes: 1,
        devices: vec![capacitor("C1", 1, 0, 1e-6)],
    };
    let bad = TransientInitialConditions {
        capacitor_voltages: vec![Some(f64::INFINITY)],
        inductor_currents: vec![],
    };
    assert!(circuit
        .transient_with_initial_conditions(
            &TransientConfig {
                t_stop: 1e-3,
                t_step: 1e-5
            },
            &bad
        )
        .is_err());
}

#[test]
fn diode_reverse_bias_converges_beyond_half_a_volt_and_satisfies_kcl() {
    let is = 2.52e-9;
    let vt = 0.02585;
    let circuit = Circuit {
        num_nodes: 2,
        devices: vec![
            vsource("VREV", 1, 0, -12.0),
            Device::Diode {
                name: "DREV".into(),
                a: 1,
                b: 2,
                is,
                vt,
            },
            resistor("RLOAD", 2, 0, 4700.0),
        ],
    };
    let result = circuit.dc().expect("12 V reverse bias should converge");
    let diode_voltage = result.node_voltages[1] - result.node_voltages[2];
    let expected_current = is * ((diode_voltage / vt).clamp(-80.0, 40.0).exp() - 1.0);

    assert!(
        diode_voltage < -11.9,
        "reverse diode voltage was {diode_voltage} V"
    );
    assert!(result.device_currents[1] < 0.0);
    assert!((result.device_currents[1] - expected_current).abs() < is * 1e-9);
    assert!((result.device_currents[1] + is).abs() < is * 1e-9);
    assert_kcl(&circuit, &result.device_currents, 1e-12);
}

#[test]
fn diode_forward_bias_converges_across_source_and_device_parameters() {
    let cases = [
        (5.0, 1_000.0, 2.52e-9, 0.02585),
        (3.3, 2_200.0, 1.0e-12, 0.02585),
        (12.0, 4_700.0, 1.0e-9, 0.026),
    ];
    for (index, (supply, resistance, is, vt)) in cases.into_iter().enumerate() {
        let circuit = Circuit {
            num_nodes: 2,
            devices: vec![
                vsource("VCC", 1, 0, supply),
                Device::Diode {
                    name: "D1".into(),
                    a: 1,
                    b: 2,
                    is,
                    vt,
                },
                resistor("R1", 2, 0, resistance),
            ],
        };
        let result = circuit.dc().unwrap_or_else(|error| {
            panic!("forward-bias test case {index} failed to converge: {error}")
        });
        let vd = result.node_voltages[1] - result.node_voltages[2];
        let expected = is * ((vd / vt).clamp(-80.0, 40.0).exp() - 1.0);
        let error = (result.device_currents[1] - expected).abs();
        assert!(error <= expected.abs().max(is) * 1e-8 + 1e-15);
        assert!(result.device_currents[1] > 0.0);
        assert!((result.device_currents[1] - result.device_currents[2]).abs() < 1e-8);
        assert_kcl(&circuit, &result.device_currents, 1e-8);
    }
}

#[test]
fn transient_diode_solver_handles_reverse_and_forward_bias() {
    for (supply, expected_sign) in [(-6.0, -1.0), (5.0, 1.0)] {
        let circuit = Circuit {
            num_nodes: 2,
            devices: vec![
                vsource("VS", 1, 0, supply),
                Device::Diode {
                    name: "D1".into(),
                    a: 1,
                    b: 2,
                    is: 2.52e-9,
                    vt: 0.02585,
                },
                resistor("R1", 2, 0, 1_000.0),
            ],
        };
        let result = circuit
            .transient(&TransientConfig {
                t_stop: 1e-4,
                t_step: 1e-5,
            })
            .unwrap_or_else(|error| panic!("transient diode case {supply} V failed: {error}"));
        for sample in 0..result.times.len() {
            let vd = result.node_voltages[sample][1] - result.node_voltages[sample][2];
            let diode_current = result.device_currents[sample][1];
            let expected = 2.52e-9 * ((vd / 0.02585).clamp(-80.0, 40.0).exp() - 1.0);
            assert_eq!(
                diode_current.signum(),
                expected_sign,
                "wrong diode current sign at sample {sample}"
            );
            assert!(
                (diode_current - expected).abs() < expected.abs().max(2.52e-9) * 1e-8 + 1e-15,
                "Shockley equation residual at sample {sample}"
            );
            assert_kcl(&circuit, &result.device_currents[sample], 1e-8);
        }
    }
}

#[test]
fn rl_natural_response_preserves_nonzero_initial_inductor_current_and_sign() {
    let circuit = Circuit {
        num_nodes: 1,
        devices: vec![resistor("R1", 1, 0, 10.0), inductor("L1", 1, 0, 0.1)],
    };
    let initial = TransientInitialConditions {
        capacitor_voltages: vec![],
        inductor_currents: vec![None, Some(1.0)],
    };
    let response = circuit
        .transient_with_initial_conditions(
            &TransientConfig {
                t_stop: 0.01,
                t_step: 1e-5,
            },
            &initial,
        )
        .unwrap();

    assert!((response.node_voltages[0][1] + 10.0).abs() < 1e-10);
    assert!((response.device_currents[0][1] - 1.0).abs() < 1e-12);
    assert!((response.device_currents[0][0] + 1.0).abs() < 1e-12);
    let final_current = *response.device_currents.last().unwrap().get(1).unwrap();
    let expected = (1.0_f64 + 1e-5 * 10.0 / 0.1).powi(-1000);
    assert!((final_current - expected).abs() < 5e-4);
    assert!((response.node_voltages.last().unwrap()[1] + 10.0 * final_current).abs() < 1e-8);
    assert_kcl(&circuit, response.device_currents.last().unwrap(), 1e-9);
}

#[test]
fn conflicting_ideal_voltage_sources_are_rejected_as_singular() {
    let circuit = Circuit {
        num_nodes: 1,
        devices: vec![vsource("V1", 1, 0, 5.0), vsource("V2", 1, 0, 3.0)],
    };
    assert!(matches!(
        circuit.dc(),
        Err(SolveError::Lu(LuError::Singular { .. }))
    ));
}

#[test]
fn capacitor_initial_voltage_conflicting_with_ideal_source_is_rejected() {
    let circuit = Circuit {
        num_nodes: 1,
        devices: vec![vsource("V1", 1, 0, 5.0), capacitor("C1", 1, 0, 1e-6)],
    };
    let initial = TransientInitialConditions {
        capacitor_voltages: vec![None, Some(3.0)],
        inductor_currents: vec![],
    };
    assert!(circuit
        .transient_with_initial_conditions(
            &TransientConfig {
                t_stop: 1e-3,
                t_step: 1e-5
            },
            &initial
        )
        .is_err());
}

#[test]
fn invalid_non_finite_sources_and_degenerate_time_values_are_rejected() {
    let non_finite_current = Circuit {
        num_nodes: 1,
        devices: vec![Device::CurrentSource {
            name: "I1".into(),
            a: 1,
            b: 0,
            amps: f64::INFINITY,
        }],
    };
    assert!(matches!(
        non_finite_current.dc(),
        Err(SolveError::BadDevice(_))
    ));

    let zero_thermal_voltage = Circuit {
        num_nodes: 1,
        devices: vec![Device::Diode {
            name: "D1".into(),
            a: 1,
            b: 0,
            is: 1e-12,
            vt: 0.0,
        }],
    };
    assert!(matches!(
        zero_thermal_voltage.dc(),
        Err(SolveError::BadDevice(_))
    ));

    let simple = Circuit {
        num_nodes: 1,
        devices: vec![vsource("V1", 1, 0, 1.0)],
    };
    assert!(simple
        .transient(&TransientConfig {
            t_stop: 1e-3,
            t_step: f64::MIN_POSITIVE,
        })
        .is_err());
}
