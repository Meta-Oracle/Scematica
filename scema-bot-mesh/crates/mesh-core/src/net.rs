//! The deterministic policy network.
//!
//! Every equation here is written against [`crate::fixed`], so a forward pass is a pure
//! function of (weights, input) with no platform-dependent behaviour anywhere in it.
//!
//! # The equations
//!
//! **Layer.**  `y = ReLU(Wx + b)` for hidden layers, linear for the output.
//!
//! **Dueling head.**  `Q(s,a) = V(s) + A(s,a) − mean_a A(s,a)`
//!
//! The subtraction of the mean advantage is not decoration. `V` and `A` are otherwise
//! unidentifiable — adding a constant to `V` and subtracting it from every `A` gives the
//! same `Q`, so training has no reason to converge on any particular split and the two
//! heads drift. Forcing the advantages to have zero mean pins the decomposition, which is
//! what makes `V(s)` separately meaningful: it becomes "how good is this state" independent
//! of which action you pick. Same formulation as `scematica-nn`'s Deep Q*, restated in
//! integer arithmetic.
//!
//! **Action selection.**  `argmax_a Q(s,a)`, ties broken by lowest index.
//!
//! Tie-breaking is specified rather than left to whatever `max_by_key` happens to do,
//! because a tie resolved differently by two implementations is a divergent action, and a
//! divergent action is a divergent game state.

use crate::fixed::{dot, mean, Fx};

/// A fully-connected layer. Row-major: `weights[o * inputs + i]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub inputs: usize,
    pub outputs: usize,
    pub weights: Vec<Fx>,
    pub biases: Vec<Fx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    Relu,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshError {
    ShapeMismatch { expected: usize, got: usize },
    EmptyNetwork,
    NoActions,
}

impl Layer {
    pub fn new(inputs: usize, outputs: usize) -> Self {
        Layer {
            inputs,
            outputs,
            weights: vec![Fx::ZERO; inputs * outputs],
            biases: vec![Fx::ZERO; outputs],
        }
    }

    /// `y = act(Wx + b)`
    pub fn forward(&self, x: &[Fx], act: Activation) -> Result<Vec<Fx>, MeshError> {
        if x.len() != self.inputs {
            return Err(MeshError::ShapeMismatch { expected: self.inputs, got: x.len() });
        }
        let mut out = Vec::with_capacity(self.outputs);
        for o in 0..self.outputs {
            let row = &self.weights[o * self.inputs..(o + 1) * self.inputs];
            let sum = dot(row, x).add(self.biases[o]);
            out.push(match act {
                Activation::Relu => sum.relu(),
                Activation::Linear => sum,
            });
        }
        Ok(out)
    }
}

/// A dueling policy network: shared trunk, then value and advantage heads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyNet {
    pub trunk: Vec<Layer>,
    /// Produces a single scalar V(s).
    pub value_head: Layer,
    /// Produces one advantage per action.
    pub advantage_head: Layer,
}

impl PolicyNet {
    /// Build with the given layer widths. `hidden` may be empty for a linear policy.
    pub fn new(state_dim: usize, hidden: &[usize], actions: usize) -> Self {
        let mut trunk = Vec::new();
        let mut width = state_dim;
        for &h in hidden {
            trunk.push(Layer::new(width, h));
            width = h;
        }
        PolicyNet {
            trunk,
            value_head: Layer::new(width, 1),
            advantage_head: Layer::new(width, actions),
        }
    }

    pub fn action_count(&self) -> usize {
        self.advantage_head.outputs
    }

    pub fn state_dim(&self) -> usize {
        self.trunk.first().map(|l| l.inputs).unwrap_or(self.value_head.inputs)
    }

    /// Q-values for a state. Pure, deterministic, allocation-order independent.
    ///
    /// `Q(s,a) = V(s) + A(s,a) − mean_a A(s,a)`
    pub fn q_values(&self, state: &[Fx]) -> Result<Vec<Fx>, MeshError> {
        if self.advantage_head.outputs == 0 {
            return Err(MeshError::NoActions);
        }

        let mut h = state.to_vec();
        for layer in &self.trunk {
            h = layer.forward(&h, Activation::Relu)?;
        }

        let value = self.value_head.forward(&h, Activation::Linear)?;
        let advantages = self.advantage_head.forward(&h, Activation::Linear)?;

        let baseline = mean(&advantages);
        let v = value[0];
        Ok(advantages.iter().map(|a| v.add(a.sub(baseline))).collect())
    }

    /// Greedy action. **Ties go to the lowest index** — specified, not incidental.
    pub fn act(&self, state: &[Fx]) -> Result<usize, MeshError> {
        let q = self.q_values(state)?;
        let mut best = 0usize;
        for (i, value) in q.iter().enumerate().skip(1) {
            if *value > q[best] {
                best = i;
            }
        }
        Ok(best)
    }

    /// Every parameter in a fixed order — trunk, then value head, then advantage head,
    /// weights before biases. This ordering *is* the serialisation format, and changing
    /// it changes every commitment hash this crate has ever produced.
    pub fn parameters(&self) -> Vec<Fx> {
        let mut out = Vec::new();
        for layer in self.trunk.iter().chain([&self.value_head, &self.advantage_head]) {
            out.extend_from_slice(&layer.weights);
            out.extend_from_slice(&layer.biases);
        }
        out
    }

    pub fn parameter_count(&self) -> usize {
        self.trunk
            .iter()
            .chain([&self.value_head, &self.advantage_head])
            .map(|l| l.weights.len() + l.biases.len())
            .sum()
    }
}

/// The Bellman target: `y = r + γ · max_a' Q(s', a')`, and `0` beyond a terminal state.
///
/// Separated out because it is the one equation a *verifier* needs in order to check that
/// a claimed training step was consistent with the transition it says it learned from.
/// Inference alone proves a policy ran; this is what makes a training claim checkable.
pub fn bellman_target(reward: Fx, gamma: Fx, next_q: &[Fx], terminal: bool) -> Fx {
    if terminal || next_q.is_empty() {
        return reward;
    }
    let mut best = next_q[0];
    for q in &next_q[1..] {
        if *q > best {
            best = *q;
        }
    }
    reward.add(gamma.mul(best))
}

/// Temporal-difference error: `δ = y − Q(s,a)`.
pub fn td_error(target: Fx, predicted: Fx) -> Fx {
    target.sub(predicted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net_2x2() -> PolicyNet {
        let mut net = PolicyNet::new(2, &[2], 2);
        // Identity-ish trunk.
        net.trunk[0].weights = vec![Fx::ONE, Fx::ZERO, Fx::ZERO, Fx::ONE];
        net.value_head.weights = vec![Fx::ONE, Fx::ONE];
        net.advantage_head.weights = vec![Fx::ONE, Fx::ZERO, Fx::ZERO, Fx::ONE];
        net
    }

    #[test]
    fn advantages_are_centred_so_v_is_identifiable() {
        // The defining property of the dueling head: mean advantage is removed, so
        // Q − V sums to zero across actions and the V/A split is pinned.
        let net = net_2x2();
        let q = net.q_values(&[Fx::from_int(3), Fx::from_int(1)]).unwrap();
        let v = Fx::from_int(4); // value head sums the two inputs
        let residual = q.iter().fold(Fx::ZERO, |acc, x| acc.add(x.sub(v)));
        assert_eq!(residual, Fx::ZERO);
    }

    #[test]
    fn adding_a_constant_to_every_advantage_changes_nothing() {
        // The identifiability argument, tested directly: shifting all advantages leaves Q
        // untouched, which is exactly why the mean has to be subtracted.
        let net = net_2x2();
        let state = [Fx::from_int(2), Fx::from_int(5)];
        let before = net.q_values(&state).unwrap();

        let mut shifted = net.clone();
        for b in shifted.advantage_head.biases.iter_mut() {
            *b = b.add(Fx::from_int(7));
        }
        assert_eq!(before, shifted.q_values(&state).unwrap());
    }

    #[test]
    fn forward_pass_is_bit_exact_on_repeat() {
        let net = net_2x2();
        let state = [Fx::from_f64(0.37), Fx::from_f64(-1.9)];
        let a = net.q_values(&state).unwrap();
        for _ in 0..64 {
            assert_eq!(net.q_values(&state).unwrap(), a);
        }
    }

    #[test]
    fn ties_break_to_the_lowest_index() {
        // Unspecified tie-breaking is a divergent action, and a divergent action is a
        // divergent game state.
        let net = PolicyNet::new(2, &[], 3); // all-zero weights -> all Q equal
        assert_eq!(net.act(&[Fx::ONE, Fx::ONE]).unwrap(), 0);
    }

    #[test]
    fn shape_mismatch_is_reported_not_panicked() {
        let net = net_2x2();
        assert_eq!(
            net.q_values(&[Fx::ONE]),
            Err(MeshError::ShapeMismatch { expected: 2, got: 1 })
        );
    }

    #[test]
    fn relu_actually_gates_the_trunk() {
        let mut net = net_2x2();
        net.trunk[0].weights = vec![Fx::ONE.neg(), Fx::ZERO, Fx::ZERO, Fx::ONE.neg()];
        // Negative pre-activations -> zeroed hidden -> value head sees nothing.
        let q = net.q_values(&[Fx::from_int(5), Fx::from_int(5)]).unwrap();
        assert!(q.iter().all(|v| *v == Fx::ZERO));
    }

    #[test]
    fn bellman_target_respects_terminality() {
        let r = Fx::from_int(2);
        let gamma = Fx::from_f64(0.9);
        let next = [Fx::from_int(10), Fx::from_int(3)];

        assert_eq!(bellman_target(r, gamma, &next, true), r, "no bootstrap past a terminal state");
        assert_eq!(bellman_target(r, gamma, &next, false), r.add(gamma.mul(Fx::from_int(10))));
        assert_eq!(bellman_target(r, gamma, &[], false), r, "no successors is terminal in effect");
    }

    #[test]
    fn td_error_is_signed_target_minus_prediction() {
        assert_eq!(td_error(Fx::from_int(5), Fx::from_int(3)), Fx::from_int(2));
        assert_eq!(td_error(Fx::from_int(3), Fx::from_int(5)), Fx::from_int(2).neg());
    }

    #[test]
    fn parameter_order_is_stable_and_complete() {
        let net = PolicyNet::new(4, &[8, 6], 3);
        assert_eq!(net.parameters().len(), net.parameter_count());
        // 4*8+8 + 8*6+6 + 6*1+1 + 6*3+3
        assert_eq!(net.parameter_count(), 40 + 54 + 7 + 21);
    }
}
