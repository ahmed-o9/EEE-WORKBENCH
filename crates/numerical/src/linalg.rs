//! Dense linear algebra with partial-pivoting LU.
//!
//! Dense LU is intentionally used for the first low-size circuit vertical
//! slice. The public trait is the boundary for a sparse implementation later.

/// Abstraction so a sparse direct solver can replace the dense backend later.
pub trait LinearSolver {
    fn solve(&mut self) -> Result<Vec<f64>, LuError>;
    fn matrix(&mut self) -> &mut [Vec<f64>];
    fn rhs(&mut self) -> &mut [f64];
}

#[derive(Debug, PartialEq)]
pub enum LuError {
    Singular { row: usize },
    NotSquare,
    DimensionMismatch,
    NonFinite { row: usize, col: usize },
}

impl std::fmt::Display for LuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LuError::Singular { row } => {
                write!(f, "matrix is singular (zero pivot at row {row})")
            }
            LuError::NotSquare => write!(f, "matrix must be square"),
            LuError::DimensionMismatch => write!(f, "matrix and right-hand-side dimensions differ"),
            LuError::NonFinite { row, col } => {
                write!(f, "matrix contains a non-finite value at ({row}, {col})")
            }
        }
    }
}
impl std::error::Error for LuError {}

/// In-place LU with partial pivoting on an n x n matrix and a dense RHS.
/// Calling `solve` consumes the current matrix state; construct a new solver
/// or restore the original coefficients before solving another RHS.
pub struct DenseLu {
    n: usize,
    pub a: Vec<Vec<f64>>,
    pub b: Vec<f64>,
}

impl DenseLu {
    pub fn new(n: usize) -> Self {
        Self {
            n,
            a: vec![vec![0.0; n]; n],
            b: vec![0.0; n],
        }
    }

    fn factorize(&mut self) -> Result<(), LuError> {
        let n = self.n;
        if self.a.len() != n || self.a.iter().any(|row| row.len() != n) {
            return Err(LuError::NotSquare);
        }
        if self.b.len() != n {
            return Err(LuError::DimensionMismatch);
        }
        for (row_index, row) in self.a.iter().enumerate() {
            for (col_index, value) in row.iter().enumerate() {
                if !value.is_finite() {
                    return Err(LuError::NonFinite {
                        row: row_index,
                        col: col_index,
                    });
                }
            }
        }
        if self.b.iter().any(|value| !value.is_finite()) {
            return Err(LuError::NonFinite { row: n, col: 0 });
        }

        for k in 0..n {
            // Partial pivot: largest absolute value in the remaining column.
            let mut pivot_row = k;
            for row in (k + 1)..n {
                if self.a[row][k].abs() > self.a[pivot_row][k].abs() {
                    pivot_row = row;
                }
            }
            // Do not use a fixed absolute epsilon here: MNA rows can contain
            // quantities with very different physical scales.
            if self.a[pivot_row][k] == 0.0 {
                return Err(LuError::Singular { row: k });
            }
            if pivot_row != k {
                self.a.swap(k, pivot_row);
                self.b.swap(k, pivot_row);
            }

            for row in (k + 1)..n {
                self.a[row][k] /= self.a[k][k];
                let multiplier = self.a[row][k];
                for col in (k + 1)..n {
                    self.a[row][col] -= multiplier * self.a[k][col];
                }
            }
        }
        Ok(())
    }
}

impl LinearSolver for DenseLu {
    fn matrix(&mut self) -> &mut [Vec<f64>] {
        &mut self.a
    }

    fn rhs(&mut self) -> &mut [f64] {
        &mut self.b
    }

    fn solve(&mut self) -> Result<Vec<f64>, LuError> {
        self.factorize()?;
        let n = self.n;

        // Forward substitution (unit lower triangle stored below the diagonal).
        for row in 0..n {
            for col in 0..row {
                self.b[row] -= self.a[row][col] * self.b[col];
            }
        }

        // Back substitution.
        let mut solution = vec![0.0; n];
        for row in (0..n).rev() {
            let mut sum = self.b[row];
            for col in (row + 1)..n {
                sum -= self.a[row][col] * solution[col];
            }
            solution[row] = sum / self.a[row][row];
            if !solution[row].is_finite() {
                return Err(LuError::NonFinite { row, col: n });
            }
        }
        Ok(solution)
    }
}
