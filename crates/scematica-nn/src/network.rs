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
/// Architecture: input → [hidden…] → output (linear).
/// All hidden layers use ReLU; output layer is linear.
#[derive(Clone, Serialize, Deserialize)]
pub struct QNetwork {
    pub layers: Vec<Linear>,
    pub layer_sizes: Vec<usize>,
}

impl QNetwork {
    pub fn new(layer_sizes: &[usize]) -> Self {
        assert!(layer_sizes.len() >= 2, "need at least input + output layer");
        let layers = layer_sizes.windows(2).map(|w| Linear::new(w[0], w[1])).collect();
        Self { layers, layer_sizes: layer_sizes.to_vec() }
    }

    pub fn forward(&self, input: &[f64]) -> Vec<f64> {
        let mut x = input.to_vec();
        let last = self.layers.len() - 1;
        for (i, layer) in self.layers.iter().enumerate() {
            let pre = layer.forward(&x);
            x = if i < last { relu(&pre) } else { pre };
        }
        x
    }

    /// Forward pass that also caches pre- and post-activation values for backprop.
    /// Returns `(output, pre_activations, post_activations)`.
    fn forward_cache(&self, input: &[f64]) -> (Vec<f64>, Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let mut pre_acts = Vec::new();
        let mut post_acts = vec![input.to_vec()];
        let mut x = input.to_vec();
        let last = self.layers.len() - 1;
        for (i, layer) in self.layers.iter().enumerate() {
            let pre = layer.forward(&x);
            pre_acts.push(pre.clone());
            x = if i < last { relu(&pre) } else { pre };
            post_acts.push(x.clone());
        }
        (x, pre_acts, post_acts)
    }

    /// One step of backprop + SGD for a single sample.
    ///
    /// Only actions indicated by `mask` contribute to the loss.
    /// Returns the masked MSE loss for this sample.
    pub fn backward_step(
        &mut self,
        input: &[f64],
        targets: &[f64],
        mask: &[bool],
        lr: f64,
        grad_clip: f64,
    ) -> f64 {
        let (output, pre_acts, post_acts) = self.forward_cache(input);

        // Output-layer gradient (MSE, masked)
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
            // Apply activation derivative for hidden layers
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

        loss / n_masked as f64
    }

    pub fn copy_from(&mut self, src: &QNetwork) {
        self.layers = src.layers.clone();
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_string(self).unwrap())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap())
    }
}
