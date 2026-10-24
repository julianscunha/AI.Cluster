//! Keys and values of every position already run, per layer.

/// Row-major `[position, kv_width]` per layer; the width is whatever the model's attention
/// stores (the full width for GPT-2, only the key/value heads for a grouped-query model).
pub struct KvCache {
    pub(crate) keys: Vec<Vec<f64>>,
    pub(crate) values: Vec<Vec<f64>>,
    pub(crate) len: usize,
}

impl KvCache {
    pub(crate) fn new(layers: usize) -> Self {
        Self {
            keys: vec![Vec::new(); layers],
            values: vec![Vec::new(); layers],
            len: 0,
        }
    }

    /// Positions held so far.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
