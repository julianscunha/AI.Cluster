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
    /// Single-threaded. Use `multiply_with_threads` to spread the same work over the
    /// cores the local scheduler granted.
    ///
    /// ponytail: naive O(n^3) triple loop in i-k-j order; a blocked kernel is the next step if the benchmark shows compute is still the bottleneck.
    pub fn multiply(&self, rhs: &Matrix) -> Option<Matrix> {
        self.multiply_with_threads(rhs, 1)
    }

    /// `self * rhs` split across up to `threads` OS threads, or `None` when the shapes
    /// do not line up.
    ///
    /// The split is by output rows and each row is still accumulated in the same i-k-j
    /// order by a single thread, so the product is bit-for-bit identical to the serial
    /// one and the cross-node checksum is unaffected — only the time to produce it
    /// changes. Threads beyond the row count get nothing to do; 0 means 1.
    pub fn multiply_with_threads(&self, rhs: &Matrix, threads: usize) -> Option<Matrix> {
        if self.cols != rhs.rows {
            return None;
        }
        let mut out = Matrix::zeros(self.rows, rhs.cols);
        let m = rhs.cols as usize;
        let rows = self.rows as usize;
        let threads = threads.max(1).min(rows.max(1));
        // `m == 0` leaves nothing to chunk; the serial path already handles it.
        if threads <= 1 || m == 0 {
            self.multiply_rows(rhs, 0, &mut out.data);
            return Some(out);
        }
        // Chunks are whole rows, so a chunk's first row is its index times the size.
        let chunk = rows.div_ceil(threads);
        std::thread::scope(|scope| {
            for (index, chunk_out) in out.data.chunks_mut(chunk * m).enumerate() {
                let row_start = index * chunk;
                scope.spawn(move || self.multiply_rows(rhs, row_start, chunk_out));
            }
        });
        Some(out)
    }

    /// Accumulates the `self * rhs` rows starting at `row_start` into `out`, one row per
    /// `rhs.cols` elements. The kernel the serial and threaded paths share, so the two
    /// cannot drift apart.
    fn multiply_rows(&self, rhs: &Matrix, row_start: usize, out: &mut [f64]) {
        let m = rhs.cols as usize;
        if m == 0 {
            return;
        }
        let k = self.cols as usize;
        for (local_row, row_out) in out.chunks_mut(m).enumerate() {
            let i = row_start + local_row;
            for p in 0..k {
                let a = self.data[i * k + p];
                if a == 0.0 {
                    continue;
                }
                let rhs_row = &rhs.data[p * m..p * m + m];
                for j in 0..m {
                    row_out[j] += a * rhs_row[j];
                }
            }
        }
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

/// Per-core matmul throughput, in GFLOP/s, that a capability score of 1.0 means.
///
/// The worker normalises its measured rate against this (`compute_score`), and the
/// controller inverts the same normalisation to turn a score back into time (H5).
/// Both ends read the constant from here so the normalisation and its inverse cannot
/// drift apart — a change on one side alone would silently mis-scale every prediction.
pub const REFERENCE_CORE_GFLOPS: f64 = 6.0;

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
    fn threading_the_product_does_not_change_it() {
        // The split is by rows and each row keeps its accumulation order, so every thread
        // count must give the identical product — checksum comparison across nodes
        // depends on it. Sizes chosen to leave a partial last chunk.
        let a = Matrix::seeded(37, 11, 5);
        let b = Matrix::seeded(11, 29, 6);
        let serial = a.multiply(&b).expect("shapes align");
        for threads in [2, 3, 4, 8, 64] {
            let threaded = a.multiply_with_threads(&b, threads).expect("shapes align");
            assert_eq!(threaded, serial, "threads={threads} changed the product");
        }
    }

    #[test]
    fn zero_threads_means_one_thread() {
        let a = Matrix::seeded(8, 8, 11);
        let b = Matrix::seeded(8, 8, 12);
        assert_eq!(
            a.multiply_with_threads(&b, 0).expect("shapes align"),
            a.multiply(&b).expect("shapes align")
        );
    }

    #[test]
    fn threaded_path_still_rejects_mismatched_shapes() {
        let a = from_rows(&[&[1.0, 2.0, 3.0]]);
        let b = from_rows(&[&[1.0, 2.0]]);
        assert!(a.multiply_with_threads(&b, 4).is_none());
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
