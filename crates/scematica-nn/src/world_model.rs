//! Latent world model — Dreamer-style imagination for sample-efficient RL.
//!
//! A world model learns the *dynamics of the market itself* so the agent can
//! train on **imagined** trajectories in addition to real ones. Real trades are
//! scarce and expensive; a learned model lets the agent "dream" thousands of
//! plausible roll-outs per real step (Dyna-style planning), which is the lever
//! that compounds the PnL edge without needing more live capital at risk.
//!
//! We use a compact, modular design (in the spirit of Ha & Schmidhuber's World
//! Models and DeepMind's Dreamer, adapted to pure-Rust f64 with no autodiff):
//!
//! ```text
//!   encoder : state(D)              → latent z(L)         (compress observation)
//!   decoder : latent z(L)           → reconstructed state (ground the latent)
//!   dynamics: [z(L), onehot(a)]     → next latent ẑ'      (imagine forward)
//!   reward  : [z(L), onehot(a)]     → r̂                   (imagine payoff)
//! ```
//!
//! Training is **modular with stop-gradients** between components (encoder+decoder
//! are trained jointly as an autoencoder; dynamics and reward train on *detached*
//! latents). This keeps every backward pass a simple chain — no cross-module
//! autodiff graph — while still yielding a grounded, predictive latent space.
//!
//! Imagination (`imagine`) rolls the dynamics + reward forward from a real start
//! state, decoding each latent back to a state vector so the produced transitions
//! live in the *same* 24-dim space the [`crate::agent::DQNAgent`] replay buffer
//! already uses — they can be pushed straight in as synthetic experience.

use crate::network::Linear;
use serde::{Deserialize, Serialize};

/// Latent dimension — the compressed market-state bottleneck.
pub const LATENT_DIM: usize = 16;
/// Hidden width for the world-model MLPs.
const HIDDEN: usize = 32;
/// Gradient clip for world-model SGD (per weight/bias).
const WM_GRAD_CLIP: f64 = 1.0;

#[inline]
fn relu(v: &[f64]) -> Vec<f64> {
    v.iter().map(|&x| x.max(0.0)).collect()
}
#[inline]
fn relu_grad(pre: &[f64]) -> Vec<f64> {
    pre.iter().map(|&x| if x > 0.0 { 1.0 } else { 0.0 }).collect()
}

fn onehot(i: usize, n: usize) -> Vec<f64> {
    let mut v = vec![0.0; n];
    if i < n {
        v[i] = 1.0;
    }
    v
}

// ── Generic feed-forward MLP (ReLU hidden, linear output) ────────────────────

/// A plain MLP used for every world-model component. Hidden layers use ReLU;
/// the final layer is linear (regression output).
#[derive(Clone, Serialize, Deserialize)]
struct Mlp {
    layers: Vec<Linear>,
}

impl Mlp {
    fn new(sizes: &[usize]) -> Self {
        assert!(sizes.len() >= 2, "MLP needs input + output");
        Self {
            layers: sizes.windows(2).map(|w| Linear::new(w[0], w[1])).collect(),
        }
    }

    fn forward(&self, input: &[f64]) -> Vec<f64> {
        let mut x = input.to_vec();
        let last = self.layers.len() - 1;
        for (i, l) in self.layers.iter().enumerate() {
            let pre = l.forward(&x);
            x = if i < last { relu(&pre) } else { pre };
        }
        x
    }

    /// Forward pass caching pre- and post-activations. `post[0]` is the input,
    /// `post[i+1]` is layer `i`'s activated output, `pre[i]` is layer `i`'s
    /// pre-activation (linear output).
    fn forward_cache(&self, input: &[f64]) -> (Vec<f64>, Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let mut pre = Vec::with_capacity(self.layers.len());
        let mut post = vec![input.to_vec()];
        let mut x = input.to_vec();
        let last = self.layers.len() - 1;
        for (i, l) in self.layers.iter().enumerate() {
            let p = l.forward(&x);
            pre.push(p.clone());
            x = if i < last { relu(&p) } else { p };
            post.push(x.clone());
        }
        (x, pre, post)
    }

    /// Backpropagate a loss gradient `delta_out` (dLoss/d output) through the
    /// network, applying SGD, and return dLoss/d input (for chaining modules).
    fn backward(
        &mut self,
        mut delta: Vec<f64>,
        pre: &[Vec<f64>],
        post: &[Vec<f64>],
        lr: f64,
    ) -> Vec<f64> {
        let n_layers = self.layers.len();
        for i in (0..n_layers).rev() {
            // Hidden layers had ReLU; the output layer is linear.
            if i < n_layers - 1 {
                let rg = relu_grad(&pre[i]);
                for (d, r) in delta.iter_mut().zip(&rg) {
                    *d *= r;
                }
            }
            let (out_sz, in_sz) = (self.layers[i].out_size, self.layers[i].in_size);
            let layer_input = &post[i];
            let mut w_grads = vec![vec![0.0; in_sz]; out_sz];
            let mut b_grads = vec![0.0; out_sz];
            let mut prev = vec![0.0; in_sz];
            for j in 0..out_sz {
                b_grads[j] = delta[j].clamp(-WM_GRAD_CLIP, WM_GRAD_CLIP);
                for k in 0..in_sz {
                    w_grads[j][k] = (delta[j] * layer_input[k]).clamp(-WM_GRAD_CLIP, WM_GRAD_CLIP);
                    prev[k] += delta[j] * self.layers[i].weights[j][k];
                }
            }
            self.layers[i].sgd_update(&w_grads, &b_grads, lr);
            delta = prev;
        }
        delta
    }

    /// One MSE gradient step against a full target vector. Returns the MSE loss.
    fn train_mse(&mut self, input: &[f64], target: &[f64], lr: f64) -> f64 {
        let (out, pre, post) = self.forward_cache(input);
        let n = out.len() as f64;
        let mut loss = 0.0;
        let delta: Vec<f64> = out
            .iter()
            .zip(target)
            .map(|(o, t)| {
                let e = o - t;
                loss += e * e;
                2.0 * e / n
            })
            .collect();
        self.backward(delta, &pre, &post, lr);
        loss / n
    }
}

// ── World model ──────────────────────────────────────────────────────────────

/// Losses returned by one world-model training step, for logging/telemetry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct WorldModelLoss {
    pub reconstruction: f64,
    pub dynamics: f64,
    pub reward: f64,
}

/// A single imagined transition, in the agent's raw state space so it can be
/// pushed directly into the replay buffer.
#[derive(Debug, Clone)]
pub struct ImaginedStep {
    pub state: Vec<f64>,
    pub action: usize,
    pub reward: f64,
    pub next_state: Vec<f64>,
}

/// Latent world model: autoencoder + latent dynamics + reward predictor.
#[derive(Clone, Serialize, Deserialize)]
pub struct WorldModel {
    encoder: Mlp,  // state_dim → HIDDEN → LATENT_DIM
    decoder: Mlp,  // LATENT_DIM → HIDDEN → state_dim
    dynamics: Mlp, // LATENT_DIM+action_dim → HIDDEN → LATENT_DIM
    reward: Mlp,   // LATENT_DIM+action_dim → HIDDEN → 1
    pub state_dim: usize,
    pub action_dim: usize,
    pub latent_dim: usize,
}

impl WorldModel {
    pub fn new(state_dim: usize, action_dim: usize) -> Self {
        let l = LATENT_DIM;
        Self {
            encoder: Mlp::new(&[state_dim, HIDDEN, l]),
            decoder: Mlp::new(&[l, HIDDEN, state_dim]),
            dynamics: Mlp::new(&[l + action_dim, HIDDEN, l]),
            reward: Mlp::new(&[l + action_dim, HIDDEN, 1]),
            state_dim,
            action_dim,
            latent_dim: l,
        }
    }

    /// Compress a state observation into its latent representation.
    pub fn encode(&self, state: &[f64]) -> Vec<f64> {
        self.encoder.forward(state)
    }

    /// Reconstruct a state vector from a latent.
    pub fn decode(&self, latent: &[f64]) -> Vec<f64> {
        self.decoder.forward(latent)
    }

    fn dyn_input(&self, latent: &[f64], action: usize) -> Vec<f64> {
        let mut v = latent.to_vec();
        v.extend(onehot(action, self.action_dim));
        v
    }

    /// One training step on a real transition `(s, a, r, s')`.
    ///
    /// - Autoencoder: minimise ‖decode(encode(s)) − s‖² (grad flows dec→enc).
    /// - Dynamics: minimise ‖dynamics(z, a) − z'‖² on **detached** latents.
    /// - Reward: minimise ‖reward(z, a) − r‖² on the detached latent.
    pub fn train(&mut self, s: &[f64], a: usize, r: f64, s_next: &[f64], lr: f64) -> WorldModelLoss {
        // ── Autoencoder (joint encoder+decoder) ──────────────────────────────
        let (z, e_pre, e_post) = self.encoder.forward_cache(s);
        let (recon, d_pre, d_post) = self.decoder.forward_cache(&z);
        let n = recon.len() as f64;
        let mut recon_loss = 0.0;
        let delta: Vec<f64> = recon
            .iter()
            .zip(s)
            .map(|(o, t)| {
                let e = o - t;
                recon_loss += e * e;
                2.0 * e / n
            })
            .collect();
        // dec backward returns dLoss/dz; chain it into the encoder.
        let z_grad = self.decoder.backward(delta, &d_pre, &d_post, lr);
        self.encoder.backward(z_grad, &e_pre, &e_post, lr);
        let recon_loss = recon_loss / n;

        // Detached latents for dynamics / reward (recompute post-update encoder).
        let z_det = self.encoder.forward(s);
        let z_next_det = self.encoder.forward(s_next);
        let inp = self.dyn_input(&z_det, a);

        let dyn_loss = self.dynamics.train_mse(&inp, &z_next_det, lr);
        let rew_loss = self.reward.train_mse(&inp, &[r], lr);

        WorldModelLoss {
            reconstruction: recon_loss,
            dynamics: dyn_loss,
            reward: rew_loss,
        }
    }

    /// Imagine a trajectory of `actions.len()` steps from `start_state`, decoding
    /// each latent back to a state vector. Returned steps are in the agent's raw
    /// state space and can be pushed straight into the replay buffer.
    pub fn imagine(&self, start_state: &[f64], actions: &[usize]) -> Vec<ImaginedStep> {
        let mut z = self.encode(start_state);
        let mut cur_state = start_state.to_vec();
        let mut out = Vec::with_capacity(actions.len());
        for &a in actions {
            let inp = self.dyn_input(&z, a);
            let r = self.reward.forward(&inp)[0];
            let z_next = self.dynamics.forward(&inp);
            let next_state = self.decode(&z_next);
            out.push(ImaginedStep {
                state: cur_state.clone(),
                action: a,
                reward: r,
                next_state: next_state.clone(),
            });
            z = z_next;
            cur_state = next_state;
        }
        out
    }

    /// One-step latent-space prediction error for `(s, a, s')` — a cheap proxy
    /// for how well the model has learned this region of dynamics (useful as an
    /// intrinsic-curiosity signal or a gate on trusting imagined rollouts).
    pub fn prediction_error(&self, s: &[f64], a: usize, s_next: &[f64]) -> f64 {
        let z = self.encode(s);
        let inp = self.dyn_input(&z, a);
        let pred = self.dynamics.forward(&inp);
        let target = self.encode(s_next);
        let n = pred.len() as f64;
        pred.iter().zip(&target).map(|(p, t)| (p - t).powi(2)).sum::<f64>() / n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_shapes() {
        let wm = WorldModel::new(24, 5);
        let z = wm.encode(&[0.5; 24]);
        assert_eq!(z.len(), LATENT_DIM);
        let recon = wm.decode(&z);
        assert_eq!(recon.len(), 24);
    }

    #[test]
    fn autoencoder_learns_to_reconstruct() {
        let mut wm = WorldModel::new(8, 5);
        let s = [0.1, 0.9, 0.3, 0.7, 0.5, 0.2, 0.8, 0.4];
        let first = wm.train(&s, 1, 0.0, &s, 1e-2).reconstruction;
        let mut last = first;
        for _ in 0..4000 {
            last = wm.train(&s, 1, 0.0, &s, 1e-2).reconstruction;
        }
        assert!(last < first * 0.5, "recon loss should fall: {first} -> {last}");
    }

    #[test]
    fn dynamics_and_reward_learn_a_fixed_transition() {
        let mut wm = WorldModel::new(6, 5);
        let s = [0.2, 0.4, 0.6, 0.8, 0.1, 0.3];
        let s_next = [0.9, 0.7, 0.5, 0.3, 0.6, 0.2];
        let a = 2usize;
        let r = 1.5;
        let mut ll = wm.train(&s, a, r, &s_next, 1e-2);
        for _ in 0..6000 {
            ll = wm.train(&s, a, r, &s_next, 1e-2);
        }
        assert!(ll.dynamics.is_finite() && ll.reward.is_finite());
        // Reward head should approach the observed reward.
        let inp = wm.dyn_input(&wm.encode(&s), a);
        let r_hat = wm.reward.forward(&inp)[0];
        assert!((r_hat - r).abs() < 0.3, "reward not learned: {r_hat} vs {r}");
        // Latent one-step error should be small after training.
        assert!(wm.prediction_error(&s, a, &s_next) < 0.1);
    }

    #[test]
    fn imagine_produces_horizon_transitions() {
        let wm = WorldModel::new(24, 5);
        let start = [0.5; 24];
        let actions = [1usize, 0, 3, 4];
        let steps = wm.imagine(&start, &actions);
        assert_eq!(steps.len(), actions.len());
        for st in &steps {
            assert_eq!(st.state.len(), 24);
            assert_eq!(st.next_state.len(), 24);
            assert!(st.reward.is_finite());
        }
        // The first imagined step starts from the real start state.
        assert_eq!(steps[0].state, start.to_vec());
    }
}
