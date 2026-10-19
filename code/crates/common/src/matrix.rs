//! Row-major f64 matrix used by the Phase 0 matrix workload.
//!
//! The workload exists to compare distributed compute against transfer cost, so
//! both generation and checksum are deterministic: two nodes must agree on the
//! result without shipping the whole product back for comparison.

#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub rows: u32,
    pub cols: u32,
    /// Row-major, `rows * cols` elements.
    pub data: Vec<f64>,
}

impl Matrix {
    pub fn zeros(rows: u32, cols: u32) -> Self {
        Self {
            rows,
            cols,
            data: vec![0.0; (rows as usize) * (cols as usize)],
        }
    }

    /// Deterministic fill from a 64-bit seed. The same seed yields the same
    /// matrix on every node, so a checksum comparison is meaningful.
    pub fn seeded(rows: u32, cols: u32, seed: u64) -> Self {
        // The golden-ratio increment makes zero a valid seed, so no normalization is needed.
        let mut state = seed;
        let data = (0..(rows as usize) * (cols as usize))
            .map(|_| {
                // ponytail: splitmix64 as a 3-line PRNG; swap for a real RNG only if distribution matters.
                state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                let unit = ((z ^ (z >> 31)) as f64) / (u64::MAX as f64);
                unit * 2.0 - 1.0
            })
            .collect();
        Self { rows, cols, data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn get(&self, row: u32, col: u32) -> f64 {
        self.data[(row as usize) * (self.cols as usize) + (col as usize)]
    }

    pub fn set(&mut self, row: u32, col: u32, value: f64) {
        let index = (row as usize) * (self.cols as usize) + (col as usize);
        self.data[index] = value;
    }

    /// `self * rhs`, or `None` when the shapes do not line up.
    ///
    /// ponytail: naive O(n^3) triple loop in i-k-j order; blocked/parallel kernels only if the benchmark shows compute is the bottleneck.
    pub fn multiply(&self, rhs: &Matrix) -> Option<Matrix> {
        if self.cols != rhs.rows {
            return None;
        }
        let n = self.rows as usize;
        let m = rhs.cols as usize;
        let k = self.cols as usize;
        let mut out = Matrix::zeros(self.rows, rhs.cols);
        for i in 0..n {
            for p in 0..k {
                let a = self.data[i * k + p];
                if a == 0.0 {
                    continue;
                }
                for j in 0..m {
                    out.data[i * m + j] += a * rhs.data[p * m + j];
                }
            }
        }
        Some(out)
    }

    /// Order-sensitive sum, used as the cross-node correctness check.
    pub fn checksum(&self) -> f64 {
        self.data.iter().sum()
    }

    /// Scale by `1/divisor`, the standard way to present GFLOP/s.
    pub fn gigaflops(&self, elapsed_secs: f64, divisor: f64) -> f64 {
        if elapsed_secs <= 0.0 {
            return 0.0;
        }
        divisor / elapsed_secs
    }
}

/// Multiply-add count for `rows x shared x cols`, matching BLAS convention.
pub fn flops(rows: u32, shared: u32, cols: u32) -> f64 {
    2.0 * f64::from(rows) * f64::from(shared) * f64::from(cols)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_rows(rows: &[&[f64]]) -> Matrix {
        let rows_u32 = rows.len() as u32;
        let cols_u32 = rows[0].len() as u32;
        let mut matrix = Matrix::zeros(rows_u32, cols_u32);
        for (i, row) in rows.iter().enumerate() {
            for (j, value) in row.iter().enumerate() {
                matrix.set(i as u32, j as u32, *value);
            }
        }
        matrix
    }

    #[test]
    fn multiplies_known_matrices() {
        let a = from_rows(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let b = from_rows(&[&[5.0, 6.0], &[7.0, 8.0]]);
        let product = a.multiply(&b).expect("shapes align");
        assert_eq!(product, from_rows(&[&[19.0, 22.0], &[43.0, 50.0]]));
    }

    #[test]
    fn rejects_mismatched_shapes() {
        let a = from_rows(&[&[1.0, 2.0, 3.0]]);
        let b = from_rows(&[&[1.0, 2.0]]);
        assert!(a.multiply(&b).is_none());
    }

    #[test]
    fn seeded_generation_is_deterministic() {
        let first = Matrix::seeded(8, 8, 42);
        let second = Matrix::seeded(8, 8, 42);
        let other = Matrix::seeded(8, 8, 43);
        assert_eq!(first, second);
        assert_ne!(first.checksum(), other.checksum());
    }

    #[test]
    fn identity_multiplication_preserves_matrix() {
        let size = 16;
        let a = Matrix::seeded(size, size, 7);
        let mut identity = Matrix::zeros(size, size);
        for i in 0..size {
            identity.set(i, i, 1.0);
        }
        let product = a.multiply(&identity).expect("shapes align");
        assert_eq!(product.checksum(), a.checksum());
        assert_eq!(flops(16, 16, 16), 8192.0);
    }
}
