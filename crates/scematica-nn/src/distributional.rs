//! Distributional value network — QR-DQN (quantile-regression Deep Q*).
//!
//! Where the standard [`crate::network::QNetwork`] predicts a single scalar
//! `Q(s,a)` — the *expected* return — a distributional network predicts the
//! **full return distribution** `Z(s,a)` as a set of learned quantiles. The
//! greedy value used for action selection is the mean of those quantiles, so it
//! is a drop-in replacement wherever `Q(s,a)` was consumed, but training against
//! the whole distribution is dramatically more sample-efficient and captures
//! risk (fat left tails from rugs) that a mean estimator smears away.
//!
//! We use **quantile regression** (Dabney et al. 2017, "Distributional RL with
//! Quantile Regression") rather than the categorical C51 parameterisation: QR
//! needs no fixed value support and no categorical projection step, which keeps
//! the from-scratch f64 backprop tractable. Each action gets `N_QUANTILES`
//! outputs `θ_i(s,a)` located at the fixed quantile midpoints
//! `τ_i = (i + 0.5) / N`. The loss is the **quantile Huber loss** summed over
//! every (predicted-quantile i, target-sample j) pair.
//!
//! Architecture keeps the dueling decomposition, generalised per-quantile:
//! ```text
//!   trunk (ReLU)  ─┬─→ value head       V:   last_hidden → N_QUANTILES
//!                  └─→ advantage head   A:   last_hidden → ACTION_DIM · N_QUANTILES
//!   Z(s,a)_i = V_i + A(a)_i − mean_b A(b)_i
//!   Q(s,a)   = (1/N) Σ_i Z(s,a)_i
//! ```
//!
//! This module is intentionally standalone: [`DQNAgent`](crate::agent::DQNAgent)
//! carries an *optional* pair of these nets, so existing scalar checkpoints keep
//! loading and running unchanged (see the `dist_*` fields on the agent).

use crate::action::ACTION_DIM;
use crate::network::Linear;
use serde::{Deserialize, Serialize};

/// Number of quantiles predicted per action. 51 mirrors the C51 atom count so
/// the resolution of the learned distribution is comparable, while remaining
/// cheap enough for per-sample f64 backprop (N·N pairwise loss ≈ 2.6k ops).
pub const N_QUANTILES: usize = 51;

/// Huber threshold κ for the quantile-Huber loss (standard QR-DQN default).
const HUBER_KAPPA: f64 = 1.0;

#[inline]
fn relu(v: &[f64]) -> Vec<f64> {
    v.iter().map(|&x| x.max(0.0)).collect()
}

#[inline]
fn relu_grad(pre: &[f64]) -> Vec<f64> {
    pre.iter().map(|&x| if x > 0.0 { 1.0 } else { 0.0 }).collect()
}

/// The fixed quantile midpoint τ_i = (i + 0.5) / N for quantile index `i`.
#[inline]
pub fn tau(i: usize, n: usize) -> f64 {
    (i as f64 + 0.5) / n as f64
}

/// Dueling quantile-regression network. Predicts `N_QUANTILES` return quantiles
/// per action; the mean over quantiles recovers the classic `Q(s,a)`.
#[derive(Clone, Serialize, Deserialize)]
pub struct QuantileNetwork {
    /// Shared trunk (all ReLU). `[input, h1, h2, …, last_hidden]`.
    pub layers: Vec<Linear>,
    /// Value head: `last_hidden → n_quantiles`.
    pub value_head: Linear,
    /// Advantage head: `last_hidden → action_dim · n_quantiles` (row-major by action).
    pub advantage_head: Linear,
    pub n_quantiles: usize,
    pub action_dim: usize,
    /// Full logical shape for checkpoint compatibility checks.
    pub layer_sizes: Vec<usize>,
}

impl QuantileNetwork {
    /// Build a dueling quantile net. `trunk_sizes` = `[state_dim, h1, h2, …]`.
    pub fn new(trunk_sizes: &[usize], action_dim: usize, n_quantiles: usize) -> Self {
        assert!(trunk_sizes.len() >= 2, "need at least input + one hidden");
        assert!(n_quantiles >= 1, "need at least one quantile");
        let layers: Vec<Linear> =
            trunk_sizes.windows(2).map(|w| Linear::new(w[0], w[1])).collect();
        let last_hidden = *trunk_sizes.last().unwrap();
        let value_head = Linear::new(last_hidden, n_quantiles);
        let advantage_head = Linear::new(last_hidden, action_dim * n_quantiles);
        Self {
            layers,
            value_head,
            advantage_head,
            n_quantiles,
            action_dim,
            layer_sizes: trunk_sizes.to_vec(),
        }
    }

    /// Standard trunk used by the agent: `[STATE_DIM, 128, 64]`, `ACTION_DIM`
    /// actions, `N_QUANTILES` quantiles.
    pub fn default_for(state_dim: usize) -> Self {
        Self::new(&[state_dim, 128, 64], ACTION_DIM, N_QUANTILES)
    }

    fn trunk_forward(&self, input: &[f64]) -> Vec<f64> {
        let mut x = input.to_vec();
        for layer in &self.layers {
            x = relu(&layer.forward(&x));
        }
        x
    }

    /// Combine value + advantage heads (already computed on the trunk output)
    /// into per-action quantile vectors: `[action_dim][n_quantiles]`.
    fn combine(&self, v: &[f64], a_flat: &[f64]) -> Vec<Vec<f64>> {
        let n = self.n_quantiles;
        let ad = self.action_dim;
        let mut mean_a = vec![0.0; n];
        for b in 0..ad {
            for i in 0..n {
                mean_a[i] += a_flat[b * n + i];
            }
        }
        for m in mean_a.iter_mut() {
            *m /= ad as f64;
        }
        (0..ad)
            .map(|b| (0..n).map(|i| v[i] + a_flat[b * n + i] - mean_a[i]).collect())
            .collect()
    }

    /// Full return distribution: `dist[a][i] = Z(s,a)_i`.
    pub fn forward_dist(&self, input: &[f64]) -> Vec<Vec<f64>> {
        let trunk = self.trunk_forward(input);
        let v = self.value_head.forward(&trunk);
        let a_flat = self.advantage_head.forward(&trunk);
        self.combine(&v, &a_flat)
    }

    /// Mean-of-quantiles `Q(s,a)` for every action — drop-in for the scalar net.
    pub fn q_values(&self, input: &[f64]) -> Vec<f64> {
        self.forward_dist(input)
            .iter()
            .map(|q| q.iter().sum::<f64>() / self.n_quantiles as f64)
            .collect()
    }

    /// Mean-of-quantiles `Q(s,a)` for a single action (avoids allocating all rows).
    pub fn q_value(&self, input: &[f64], action: usize) -> f64 {
        let dist = self.forward_dist(input);
        let row = &dist[action.min(self.action_dim - 1)];
        row.iter().sum::<f64>() / self.n_quantiles as f64
    }

    /// Argmax action by mean-of-quantiles.
    pub fn best_action(&self, input: &[f64]) -> usize {
        let q = self.q_values(input);
        q.iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// One quantile-Huber gradient step for a single transition.
    ///
    /// Trains only the quantiles of `action` against `target_quantiles` (the
    /// Bellman-backed target distribution `r + γ·Z_target(s',a*)`, or just `r`
    /// repeated `N` times when terminal). Returns the mean quantile-Huber loss.
    ///
    /// `is_weight` scales the whole gradient (prioritized-replay importance
    /// sampling correction), matching the scalar net's `backward_step` contract.
    pub fn train_quantile(
        &mut self,
        input: &[f64],
        action: usize,
        target_quantiles: &[f64],
        lr: f64,
        is_weight: f64,
    ) -> f64 {
        let n = self.n_quantiles;
        let ad = self.action_dim;
        debug_assert_eq!(target_quantiles.len(), n);

        // ── Forward with cached trunk activations ────────────────────────────
        let mut pre_acts: Vec<Vec<f64>> = Vec::with_capacity(self.layers.len());
        let mut post_acts: Vec<Vec<f64>> = vec![input.to_vec()];
        let mut x = input.to_vec();
        for layer in &self.layers {
            let pre = layer.forward(&x);
            pre_acts.push(pre.clone());
            x = relu(&pre);
            post_acts.push(x.clone());
        }
        let trunk_out = x;
        let v = self.value_head.forward(&trunk_out);
        let a_flat = self.advantage_head.forward(&trunk_out);

        // Mean advantage per quantile (for the dueling coupling term).
        let mut mean_a = vec![0.0; n];
        for b in 0..ad {
            for i in 0..n {
                mean_a[i] += a_flat[b * n + i];
            }
        }
        for m in mean_a.iter_mut() {
            *m /= ad as f64;
        }

        // Current quantiles θ_i for the trained action.
        let theta: Vec<f64> =
            (0..n).map(|i| v[i] + a_flat[action * n + i] - mean_a[i]).collect();

        // ── Quantile-Huber loss and dL/dθ_i ──────────────────────────────────
        // For each predicted quantile i (at τ_i) vs each target sample j:
        //   u        = T_j − θ_i
        //   huber(u) = ½u²                    if |u| ≤ κ
        //              κ(|u| − ½κ)            otherwise
        //   ρ_ij     = |τ_i − 1{u<0}| · huber(u) / κ
        //   dρ/dθ_i  = −|τ_i − 1{u<0}| · clip(u, −κ, κ) / κ     (du/dθ_i = −1)
        let mut loss = 0.0;
        let mut delta = vec![0.0; n]; // dLoss/dθ_i
        for i in 0..n {
            let t = tau(i, n);
            let mut g = 0.0;
            for &tj in target_quantiles.iter() {
                let u = tj - theta[i];
                let huber = if u.abs() <= HUBER_KAPPA {
                    0.5 * u * u
                } else {
                    HUBER_KAPPA * (u.abs() - 0.5 * HUBER_KAPPA)
                };
                let w = (t - if u < 0.0 { 1.0 } else { 0.0 }).abs();
                loss += w * huber / HUBER_KAPPA;
                let hgrad = u.clamp(-HUBER_KAPPA, HUBER_KAPPA); // dHuber/du
                g += w * (-hgrad) / HUBER_KAPPA;
            }
            delta[i] = is_weight * g / n as f64; // mean over target samples
        }
        loss /= (n * n) as f64;

        // ── Head gradients (dueling coupling) ────────────────────────────────
        // dL/dV_i        = delta_i
        // dL/dA(b)_i     = delta_i · (1{b==action} − 1/ad)
        let inv_ad = 1.0 / ad as f64;
        let g_v = &delta; // length n
        let mut g_a = vec![0.0; ad * n];
        for b in 0..ad {
            let coeff = if b == action { 1.0 - inv_ad } else { -inv_ad };
            for i in 0..n {
                g_a[b * n + i] = delta[i] * coeff;
            }
        }

        // Backprop through advantage head → trunk_delta.
        let mut trunk_delta = vec![0.0; trunk_out.len()];
        Self::linear_backward(
            &mut self.advantage_head,
            &trunk_out,
            &g_a,
            lr,
            &mut trunk_delta,
        );
        // Backprop through value head → accumulate into trunk_delta.
        Self::linear_backward(&mut self.value_head, &trunk_out, g_v, lr, &mut trunk_delta);

        // ── Backprop through the ReLU trunk ──────────────────────────────────
        let n_layers = self.layers.len();
        let mut d = trunk_delta;
        for l in (0..n_layers).rev() {
            let rg = relu_grad(&pre_acts[l]);
            for (di, r) in d.iter_mut().zip(&rg) {
                *di *= r;
            }
            let layer_input = &post_acts[l];
            let mut prev = vec![0.0; self.layers[l].in_size];
            let (out_sz, in_sz) = (self.layers[l].out_size, self.layers[l].in_size);
            let mut w_grads = vec![vec![0.0; in_sz]; out_sz];
            let mut b_grads = vec![0.0; out_sz];
            for j in 0..out_sz {
                b_grads[j] = d[j];
                for k in 0..in_sz {
                    w_grads[j][k] = d[j] * layer_input[k];
                    prev[k] += d[j] * self.layers[l].weights[j][k];
                }
            }
            self.layers[l].sgd_update(&w_grads, &b_grads, lr);
            d = prev;
        }

        loss
    }

    /// Backprop one linear layer: applies SGD to `layer` and accumulates the
    /// input-side gradient into `input_delta`. `out_grad` is dLoss/d(layer output).
    fn linear_backward(
        layer: &mut Linear,
        input: &[f64],
        out_grad: &[f64],
        lr: f64,
        input_delta: &mut [f64],
    ) {
        let (out_sz, in_sz) = (layer.out_size, layer.in_size);
        let mut w_grads = vec![vec![0.0; in_sz]; out_sz];
        let mut b_grads = vec![0.0; out_sz];
        for j in 0..out_sz {
            b_grads[j] = out_grad[j];
            for k in 0..in_sz {
                w_grads[j][k] = out_grad[j] * input[k];
                input_delta[k] += out_grad[j] * layer.weights[j][k];
            }
        }
        layer.sgd_update(&w_grads, &b_grads, lr);
    }

    pub fn copy_from(&mut self, src: &QuantileNetwork) {
        self.layers = src.layers.clone();
        self.value_head = src.value_head.clone();
        self.advantage_head = src.advantage_head.clone();
        self.n_quantiles = src.n_quantiles;
        self.action_dim = src.action_dim;
        self.layer_sizes = src.layer_sizes.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_shapes_are_correct() {
        let net = QuantileNetwork::new(&[8, 16, 8], ACTION_DIM, N_QUANTILES);
        let dist = net.forward_dist(&[0.5; 8]);
        assert_eq!(dist.len(), ACTION_DIM);
        assert!(dist.iter().all(|row| row.len() == N_QUANTILES));
        let q = net.q_values(&[0.5; 8]);
        assert_eq!(q.len(), ACTION_DIM);
        // q_value(single) must equal the mean of that action's row.
        for a in 0..ACTION_DIM {
            let mean = dist[a].iter().sum::<f64>() / N_QUANTILES as f64;
            assert!((net.q_value(&[0.5; 8], a) - mean).abs() < 1e-9);
        }
    }

    #[test]
    fn dueling_mean_advantage_is_zero() {
        // By construction mean_b A(b)_i is subtracted, so the per-quantile mean
        // of the advantage contribution across actions is 0: mean_a Z(s,a)_i == V_i.
        let net = QuantileNetwork::new(&[6, 12, 6], ACTION_DIM, N_QUANTILES);
        let dist = net.forward_dist(&[0.3; 6]);
        for i in 0..N_QUANTILES {
            let mean_over_actions: f64 =
                (0..ACTION_DIM).map(|a| dist[a][i]).sum::<f64>() / ACTION_DIM as f64;
            let trunk = net.trunk_forward(&[0.3; 6]);
            let v_i = net.value_head.forward(&trunk)[i];
            assert!((mean_over_actions - v_i).abs() < 1e-9);
        }
    }

    #[test]
    fn training_fits_a_constant_target_distribution() {
        // If every target sample is the constant c, the optimal prediction is
        // that all quantiles collapse to c, so Q(s,a) → c.
        let mut net = QuantileNetwork::new(&[4, 32, 16], ACTION_DIM, N_QUANTILES);
        let input = [0.2, 0.4, 0.6, 0.8];
        let action = 2usize;
        let c = 3.0;
        let target = vec![c; N_QUANTILES];

        let first = net.train_quantile(&input, action, &target, 1e-2, 1.0);
        let mut last = first;
        for _ in 0..3000 {
            last = net.train_quantile(&input, action, &target, 1e-2, 1.0);
        }
        assert!(last < first, "loss should decrease: {first} -> {last}");
        let q = net.q_value(&input, action);
        assert!((q - c).abs() < 0.25, "Q({action}) should approach {c}, got {q}");
    }

    #[test]
    fn quantiles_learn_monotone_spread_of_a_distribution() {
        // Train against a two-point target {0, 10}. The learned quantiles should
        // span the range: low quantiles near 0, high quantiles near 10, and be
        // (weakly) monotonically increasing in τ.
        let mut net = QuantileNetwork::new(&[3, 32, 16], ACTION_DIM, N_QUANTILES);
        let input = [0.1, 0.5, 0.9];
        let action = 1usize;
        let mut target = vec![0.0; N_QUANTILES];
        for (i, t) in target.iter_mut().enumerate() {
            *t = if i < N_QUANTILES / 2 { 0.0 } else { 10.0 };
        }
        for _ in 0..5000 {
            net.train_quantile(&input, action, &target, 5e-3, 1.0);
        }
        let dist = net.forward_dist(&input);
        let row = &dist[action];
        // Lowest quantile well below the highest.
        assert!(row[0] < row[N_QUANTILES - 1] - 3.0, "spread not learned: {row:?}");
        // Mean should sit between the two modes.
        let mean = row.iter().sum::<f64>() / N_QUANTILES as f64;
        assert!(mean > 2.0 && mean < 8.0, "mean {mean} not between modes");
    }

    #[test]
    fn copy_from_replicates_outputs() {
        let src = QuantileNetwork::new(&[5, 10, 5], ACTION_DIM, N_QUANTILES);
        let mut dst = QuantileNetwork::new(&[5, 10, 5], ACTION_DIM, N_QUANTILES);
        dst.copy_from(&src);
        let a = src.q_values(&[0.7; 5]);
        let b = dst.q_values(&[0.7; 5]);
        assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-12));
    }
}
