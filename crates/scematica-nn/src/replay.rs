use rand::Rng;
use serde::{Deserialize, Serialize};

/// A single (s, a, r, s', done) experience tuple.
#[derive(Clone, Serialize, Deserialize)]
pub struct Transition {
    pub state: Vec<f64>,
    pub action: usize,
    pub reward: f64,
    pub next_state: Vec<f64>,
    /// True when the episode ended (e.g. position closed, daily limit hit).
    pub done: bool,
}

/// A sampled batch with transition data, buffer indices, and IS weights.
pub struct PrioritizedBatch {
    pub transitions: Vec<Transition>,
    /// Indices into the ring buffer (used to update priorities after training)
    pub indices: Vec<usize>,
    /// Importance-sampling weights (used to correct for non-uniform sampling bias)
    pub weights: Vec<f64>,
}

/// Power-of-two sum-tree for O(log n) priority updates and proportional sampling.
/// Layout: 1-indexed; root at 1, children of i at 2i / 2i+1, leaves at capacity..2*capacity-1.
struct SumTree {
    data: Vec<f64>,
    capacity: usize,
}

impl SumTree {
    fn new(capacity: usize) -> Self {
        // Round up to nearest power-of-two so the tree is complete
        let cap = capacity.next_power_of_two();
        Self { data: vec![0.0; 2 * cap], capacity: cap }
    }

    fn total(&self) -> f64 { self.data[1] }

    fn update(&mut self, leaf: usize, priority: f64) {
        let mut idx = self.capacity + leaf;
        let diff = priority - self.data[idx];
        self.data[idx] = priority;
        idx >>= 1;
        while idx >= 1 {
            self.data[idx] += diff;
            if idx == 1 { break; }
            idx >>= 1;
        }
    }

    /// Find the leaf whose prefix sum contains `value`.
    fn find(&self, mut value: f64) -> usize {
        let mut idx = 1;
        while idx < self.capacity {
            let left = idx * 2;
            let right = left + 1;
            if value <= self.data[left] {
                idx = left;
            } else {
                value -= self.data[left];
                idx = right;
            }
        }
        idx - self.capacity
    }

    fn get(&self, leaf: usize) -> f64 {
        self.data[self.capacity + leaf]
    }
}

/// Prioritized Experience Replay buffer.
///
/// Uses a sum-tree for O(log n) priority-proportional sampling.
/// New transitions receive `max_priority` so they are always trained on at least once.
///
/// - `alpha` (0.6): how much prioritization is used (0 = uniform, 1 = full PER).
/// - `beta` (0.4 → 1.0): importance-sampling exponent that corrects for sampling bias.
pub struct PrioritizedReplayBuffer {
    transitions: Vec<Option<Transition>>,
    tree: SumTree,
    capacity: usize,
    write_head: usize,
    len: usize,
    max_priority: f64,
    alpha: f64,
    beta: f64,
    beta_increment: f64,
    /// Small constant added to each priority to ensure non-zero probability
    epsilon: f64,
}

impl PrioritizedReplayBuffer {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two();
        Self {
            transitions: vec![None; cap],
            tree: SumTree::new(cap),
            capacity: cap,
            write_head: 0,
            len: 0,
            max_priority: 1.0,
            alpha: 0.6,
            beta: 0.4,
            beta_increment: 0.001,
            epsilon: 1e-6,
        }
    }

    pub fn push(&mut self, t: Transition) {
        let idx = self.write_head;
        self.transitions[idx] = Some(t);
        self.tree.update(idx, self.max_priority.powf(self.alpha));
        self.write_head = (self.write_head + 1) % self.capacity;
        self.len = (self.len + 1).min(self.capacity);
    }

    /// Sample `n` transitions with priority-proportional probability.
    /// Returns a `PrioritizedBatch` containing transitions, their buffer indices,
    /// and importance-sampling weights normalised to [0, 1].
    pub fn sample(&mut self, n: usize) -> PrioritizedBatch {
        let n = n.min(self.len);
        let mut transitions = Vec::with_capacity(n);
        let mut indices = Vec::with_capacity(n);
        let mut weights = Vec::with_capacity(n);

        let total = self.tree.total();
        let segment = total / n as f64;
        let min_prob = self.tree.get(self.min_priority_leaf()) / total;
        let max_weight = (self.len as f64 * min_prob).powf(-self.beta);

        let mut rng = rand::thread_rng();
        for i in 0..n {
            let lo = segment * i as f64;
            let hi = segment * (i + 1) as f64;
            let value = rng.gen_range(lo..hi.min(total - 1e-10));
            let leaf = self.tree.find(value);
            let leaf = leaf.min(self.capacity - 1);

            // Skip empty slots (can happen near write_head at startup)
            if self.transitions[leaf].is_none() {
                transitions.push(Transition {
                    state: vec![],
                    action: 0,
                    reward: 0.0,
                    next_state: vec![],
                    done: true,
                });
                indices.push(leaf);
                weights.push(1.0);
                continue;
            }

            let priority = self.tree.get(leaf).max(self.epsilon);
            let prob = priority / total;
            let w = ((self.len as f64 * prob).powf(-self.beta) / max_weight).min(1.0);

            transitions.push(self.transitions[leaf].clone().unwrap());
            indices.push(leaf);
            weights.push(w);
        }

        // Anneal beta towards 1.0
        self.beta = (self.beta + self.beta_increment).min(1.0);

        PrioritizedBatch { transitions, indices, weights }
    }

    /// Update priorities for a batch of transitions after computing their TD errors.
    pub fn update_priorities(&mut self, indices: &[usize], td_errors: &[f64]) {
        for (&idx, &err) in indices.iter().zip(td_errors.iter()) {
            let priority = (err.abs() + self.epsilon).powf(self.alpha);
            self.tree.update(idx, priority);
            if priority > self.max_priority {
                self.max_priority = priority;
            }
        }
    }

    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }

    fn min_priority_leaf(&self) -> usize {
        let mut min_leaf = 0;
        let mut min_val = f64::MAX;
        for i in 0..self.len {
            let v = self.tree.get(i);
            if v < min_val && v > 0.0 {
                min_val = v;
                min_leaf = i;
            }
        }
        min_leaf
    }
}
