use rand::Rng;
use serde::{Deserialize, Serialize};

// ── Dense layer ─────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct Linear {
    /// Weight matrix [out_size][in_size] — row-major.
    pub weights: Vec<Vec<f64>>,
    pub biases: Vec<f64>,
    pub in_size: usize,
    pub out_size: usize,
}

impl Linear {
    /// He initialisation: w ~ Uniform(-sqrt(2/in), +sqrt(2/in)).
    pub fn new(in_size: usize, out_size: usize) -> Self {
        let mut rng = rand::thread_rng();
        let bound = (2.0_f64 / in_size as f64).sqrt();
        let weights = (0..out_size)
            .map(|_| (0..in_size).map(|_| rng.gen_range(-bound..bound)).collect())
            .collect();
        Self { weights, biases: vec![0.0; out_size], in_size, out_size }
    }

    pub fn forward(&self, input: &[f64]) -> Vec<f64> {
        (0..self.out_size)
            .map(|i| {
                self.biases[i]
                    + self.weights[i]
                        .iter()
                        .zip(input)
                        .map(|(w, x)| w * x)
                        .sum::<f64>()
            })
            .collect()
    }

    pub fn sgd_update(&mut self, w_grads: &[Vec<f64>], b_grads: &[f64], lr: f64) {
        for i in 0..self.out_size {
            self.biases[i] -= lr * b_grads[i];
            for j in 0..self.in_size {
                self.weights[i][j] -= lr * w_grads[i][j];
            }
        }
    }
}

// ── Activations ─────────────────────────────────────────────────────────────

#[inline]
fn relu(v: &[f64]) -> Vec<f64> { v.iter().map(|&x| x.max(0.0)).collect() }

#[inline]
fn relu_grad(pre: &[f64]) -> Vec<f64> {
    pre.iter().map(|&x| if x > 0.0 { 1.0 } else { 0.0 }).collect()
}

// ── Q-Network (MLP) ─────────────────────────────────────────────────────────

/// Multi-layer perceptron that approximates the Q* function.
///
/// Supports two architectures:
/// - **Standard**: input → [hidden…] → Q(s,a)  (value_head/advantage_head are None)
/// - **Dueling DQN**: shared layers → V(s) head AND A(s,a) head.
///   Q(s,a) = V(s) + A(s,a) - mean_a(A(s,a))
///   The mean-subtraction prevents A(s,a) from being unidentifiable with V(s).
///
/// Old checkpoints that lack `value_head`/`advantage_head` deserialise cleanly
/// because both fields are `#[serde(default)]` (Option → None = standard mode).
#[derive(Clone, Serialize, Deserialize)]
pub struct QNetwork {
    /// Shared trunk layers (all except the final output layer in standard mode,
    /// or all hidden layers in dueling mode).
    pub layers: Vec<Linear>,
    pub layer_sizes: Vec<usize>,
    /// Dueling value head: last_hidden_dim → 1.  None = standard Q-network.
    #[serde(default)]
    pub value_head: Option<Linear>,
    /// Dueling advantage head: last_hidden_dim → ACTION_DIM.  None = standard.
    #[serde(default)]
    pub advantage_head: Option<Linear>,
}

impl QNetwork {
    /// Standard Q-network (MLP without dueling).
    pub fn new(layer_sizes: &[usize]) -> Self {
        assert!(layer_sizes.len() >= 2, "need at least input + output layer");
        let layers = layer_sizes.windows(2).map(|w| Linear::new(w[0], w[1])).collect();
        Self { layers, layer_sizes: layer_sizes.to_vec(), value_head: None, advantage_head: None }
    }

    /// Dueling DQN: shared trunk [input → hidden…] + separate V and A heads.
    /// `shared_sizes` should be [input_dim, h1, h2, …, last_hidden_dim].
    /// `action_dim` is the number of discrete actions.
    pub fn new_dueling(shared_sizes: &[usize], action_dim: usize) -> Self {
        assert!(shared_sizes.len() >= 2, "need at least input + one hidden");
        let layers = shared_sizes.windows(2).map(|w| Linear::new(w[0], w[1])).collect();
        let last_hidden = *shared_sizes.last().unwrap();
        let value_head = Linear::new(last_hidden, 1);
        let advantage_head = Linear::new(last_hidden, action_dim);
        // layer_sizes stores the full logical shape for checkpoint compat
        let mut ls = shared_sizes.to_vec();
        ls.push(action_dim);
        Self { layers, layer_sizes: ls, value_head: Some(value_head), advantage_head: Some(advantage_head) }
    }

    pub fn forward(&self, input: &[f64]) -> Vec<f64> {
        let mut x = input.to_vec();
        let is_dueling = self.value_head.is_some() && self.advantage_head.is_some();

        if is_dueling {
            // In dueling mode: all trunk layers use ReLU (outputs come from heads).
            for layer in &self.layers {
                let pre = layer.forward(&x);
                x = relu(&pre);
            }
            let vh = self.value_head.as_ref().unwrap();
            let ah = self.advantage_head.as_ref().unwrap();
            let v = vh.forward(&x)[0];
            let a = ah.forward(&x);
            let mean_a = a.iter().sum::<f64>() / a.len() as f64;
            a.iter().map(|&ai| v + ai - mean_a).collect()
        } else {
            // Standard mode: ReLU on all but the last layer (which is the linear output).
            let last = self.layers.len().saturating_sub(1);
            for (i, layer) in self.layers.iter().enumerate() {
                let pre = layer.forward(&x);
                x = if i < last { relu(&pre) } else { pre };
            }
            x
        }
    }

    /// Forward pass that caches intermediate activations for backprop.
    /// Returns `(output, pre_activations, post_activations)`.
    /// In dueling mode, `output` is the combined Q-values and caches include trunk only.
    fn forward_cache(&self, input: &[f64]) -> (Vec<f64>, Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let mut pre_acts = Vec::new();
        let mut post_acts = vec![input.to_vec()];
        let mut x = input.to_vec();
        let is_dueling = self.value_head.is_some() && self.advantage_head.is_some();
        let last = self.layers.len().saturating_sub(1);

        for (i, layer) in self.layers.iter().enumerate() {
            let pre = layer.forward(&x);
            pre_acts.push(pre.clone());
            // Dueling: all trunk layers use ReLU. Standard: last is linear.
            x = if is_dueling || i < last { relu(&pre) } else { pre };
            post_acts.push(x.clone());
        }

        let output = if is_dueling {
            let vh = self.value_head.as_ref().unwrap();
            let ah = self.advantage_head.as_ref().unwrap();
            let v = vh.forward(&x)[0];
            let a = ah.forward(&x);
            let mean_a = a.iter().sum::<f64>() / a.len() as f64;
            a.iter().map(|&ai| v + ai - mean_a).collect()
        } else {
            x
        };

        (output, pre_acts, post_acts)
    }

    /// One step of backprop + SGD for a single sample.
    ///
    /// Only actions indicated by `mask` contribute to the loss.
    /// Returns the masked MSE loss for this sample.
    /// Handles both standard and dueling architectures.
    pub fn backward_step(
        &mut self,
        input: &[f64],
        targets: &[f64],
        mask: &[bool],
        lr: f64,
        grad_clip: f64,
    ) -> f64 {
        let is_dueling = self.value_head.is_some() && self.advantage_head.is_some();

        // ── Standard mode backprop ───────────────────────────────────────────
        if !is_dueling {
            let (output, pre_acts, post_acts) = self.forward_cache(input);
            let mut loss = 0.0;
            let n_masked = mask.iter().filter(|&&m| m).count().max(1);
            let mut delta: Vec<f64> = output
                .iter()
                .zip(targets)
                .zip(mask)
                .map(|((o, t), &m)| {
                    if m {
                        let err = o - t;
                        loss += err * err;
                        2.0 * err / n_masked as f64
                    } else {
                        0.0
                    }
                })
                .collect();

            let n_layers = self.layers.len();
            for i in (0..n_layers).rev() {
                if i < n_layers - 1 {
                    let rg = relu_grad(&pre_acts[i]);
                    for (d, r) in delta.iter_mut().zip(&rg) { *d *= r; }
                }
                let out_sz = self.layers[i].out_size;
                let in_sz  = self.layers[i].in_size;
                let layer_input = &post_acts[i];
                let mut w_grads = vec![vec![0.0; in_sz]; out_sz];
                let mut b_grads = vec![0.0; out_sz];
                let mut prev_delta = vec![0.0; in_sz];
                for j in 0..out_sz {
                    b_grads[j] = delta[j].clamp(-grad_clip, grad_clip);
                    for k in 0..in_sz {
                        let g = (delta[j] * layer_input[k]).clamp(-grad_clip, grad_clip);
                        w_grads[j][k] = g;
                        prev_delta[k] += delta[j] * self.layers[i].weights[j][k];
                    }
                }
                self.layers[i].sgd_update(&w_grads, &b_grads, lr);
                delta = prev_delta;
            }
            return loss / n_masked as f64;
        }

        // ── Dueling mode backprop ────────────────────────────────────────────
        // Q(s,a) = V(s) + A(s,a) - mean_a(A(s,a))
        // dL/dA_a = dL/dQ_a - (1/n) * sum_b(dL/dQ_b)
        // dL/dV   = sum_b(dL/dQ_b)

        let (output, trunk_pre, trunk_post) = self.forward_cache(input);
        let trunk_out = trunk_post.last().unwrap().clone();

        // Compute dL/dQ for each action
        let mut loss = 0.0;
        let n_masked = mask.iter().filter(|&&m| m).count().max(1);
        let n_actions = output.len();
        let dq: Vec<f64> = output.iter().zip(targets).zip(mask).map(|((o, t), &m)| {
            if m {
                let err = o - t;
                loss += err * err;
                2.0 * err / n_masked as f64
            } else {
                0.0
            }
        }).collect();

        let sum_dq: f64 = dq.iter().sum();
        let inv_n = 1.0 / n_actions as f64;

        // Gradient wrt advantage head outputs
        let da: Vec<f64> = dq.iter().map(|&dqi| dqi - inv_n * sum_dq).collect();
        // Gradient wrt value head output (scalar)
        let dv = sum_dq;

        // Update advantage head
        {
            let ah = self.advantage_head.as_mut().unwrap();
            let in_sz = ah.in_size;
            let out_sz = ah.out_size;
            let mut w_grads = vec![vec![0.0; in_sz]; out_sz];
            let mut b_grads = vec![0.0; out_sz];
            let mut trunk_delta = vec![0.0; in_sz];
            for j in 0..out_sz {
                b_grads[j] = da[j].clamp(-grad_clip, grad_clip);
                for k in 0..in_sz {
                    let g = (da[j] * trunk_out[k]).clamp(-grad_clip, grad_clip);
                    w_grads[j][k] = g;
                    trunk_delta[k] += da[j] * ah.weights[j][k];
                }
            }
            ah.sgd_update(&w_grads, &b_grads, lr);

            // Update value head
            let vh = self.value_head.as_mut().unwrap();
            let vin_sz = vh.in_size;
            let mut vw_grads = vec![vec![0.0; vin_sz]; 1];
            let mut vb_grads = vec![0.0; 1];
            vb_grads[0] = dv.clamp(-grad_clip, grad_clip);
            for k in 0..vin_sz {
                let g = (dv * trunk_out[k]).clamp(-grad_clip, grad_clip);
                vw_grads[0][k] = g;
                trunk_delta[k] += dv * vh.weights[0][k];
            }
            vh.sgd_update(&vw_grads, &vb_grads, lr);

            // Propagate combined gradient through trunk layers
            let n_layers = self.layers.len();
            let mut delta = trunk_delta;
            for i in (0..n_layers).rev() {
                // All trunk layers have ReLU in dueling mode
                let rg = relu_grad(&trunk_pre[i]);
                for (d, r) in delta.iter_mut().zip(&rg) { *d *= r; }
                let out_sz = self.layers[i].out_size;
                let in_sz  = self.layers[i].in_size;
                let layer_input = &trunk_post[i];
                let mut w_grads = vec![vec![0.0; in_sz]; out_sz];
                let mut b_grads = vec![0.0; out_sz];
                let mut prev_delta = vec![0.0; in_sz];
                for j in 0..out_sz {
                    b_grads[j] = delta[j].clamp(-grad_clip, grad_clip);
                    for k in 0..in_sz {
                        let g = (delta[j] * layer_input[k]).clamp(-grad_clip, grad_clip);
                        w_grads[j][k] = g;
                        prev_delta[k] += delta[j] * self.layers[i].weights[j][k];
                    }
                }
                self.layers[i].sgd_update(&w_grads, &b_grads, lr);
                delta = prev_delta;
            }
        }

        loss / n_masked as f64
    }

    pub fn copy_from(&mut self, src: &QNetwork) {
        self.layers = src.layers.clone();
        self.value_head = src.value_head.clone();
        self.advantage_head = src.advantage_head.clone();
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_string(self).unwrap())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap())
    }
}
