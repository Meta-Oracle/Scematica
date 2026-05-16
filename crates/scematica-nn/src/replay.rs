use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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

/// Fixed-capacity ring-buffer replay memory.
pub struct ReplayBuffer {
    buffer: VecDeque<Transition>,
    capacity: usize,
}

impl ReplayBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { buffer: VecDeque::with_capacity(capacity), capacity }
    }

    pub fn push(&mut self, t: Transition) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(t);
    }

    /// Uniform random sample without replacement.
    pub fn sample(&self, n: usize) -> Vec<Transition> {
        let mut rng = rand::thread_rng();
        let buf: Vec<&Transition> = self.buffer.iter().collect();
        buf.choose_multiple(&mut rng, n.min(buf.len()))
            .map(|t| (*t).clone())
            .collect()
    }

    pub fn len(&self) -> usize { self.buffer.len() }
    pub fn is_empty(&self) -> bool { self.buffer.is_empty() }
}
