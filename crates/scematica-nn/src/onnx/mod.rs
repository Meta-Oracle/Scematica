//! Export a trained Q-network to a real `.onnx` file.
//!
//! The agent trains in this crate and nowhere else, which makes it invisible to every
//! tool that speaks ONNX — runtimes, profilers, quantisers, Netron, and any serving path
//! that is not a Rust binary linking this crate. Export closes that off: the weights
//! learned from live trading become a portable graph anything can execute.
//!
//! # The graph
//!
//! [`QNetwork`] has two architectures and both are emitted faithfully.
//!
//! **Dueling** — trunk, then two heads recombined:
//!
//! ```text
//!   state[batch, 24]
//!     ├─ Gemm(Wᵢ, bᵢ, transB=1) → Relu     for each trunk layer
//!     ├─ Gemm(W_v, b_v) ───────────────────→ V [batch, 1]
//!     └─ Gemm(W_a, b_a) ───────────────────→ A [batch, 5]
//!                                             ├─ ReduceMean(axes=[1], keepdims=1) → Ā
//!                                             ├─ Sub(A, Ā)
//!                                             └─ Add(·, V) → q_values[batch, 5]
//! ```
//!
//! That last trio is the identity `Q(s,a) = V(s) + A(s,a) − mean_a A(s,a)`, which is the
//! whole point of the dueling architecture: without the mean subtraction, `V` and `A` are
//! unidentifiable — any constant can move between them — and the split stops meaning
//! anything. Exporting it as three ops rather than folding it into the weights keeps the
//! decomposition legible in Netron and keeps `V` readable as its own output.
//!
//! **Standard** — ReLU on every layer but the last, which stays linear because Q-values
//! are unbounded and a ReLU on the output would clamp every negative Q to zero.
//!
//! # Weight layout
//!
//! [`Linear`] stores `weights[out][in]` and computes `out_i = b_i + Σ_j W_ij x_j`. ONNX
//! `Gemm` computes `Y = α·A·B + β·C`, so with `A = x [batch, in]` the matrix must be
//! `[in, out]` — the transpose of how it is stored. Rather than transpose every matrix on
//! export, the nodes set `transB=1` and pass `W` exactly as it sits in memory. Fewer
//! copies, and no opportunity to transpose one matrix and forget another.
//!
//! # Precision
//!
//! Training is `f64`; the tensors are written as `f32`, which every runtime supports and
//! which halves the file. That narrowing is the only lossy step, and it is verified
//! rather than assumed — see `scripts/validate_onnx.py`, which runs the exported graph
//! under onnxruntime and asserts the Q-values match this crate's forward pass.

pub mod protobuf;

use crate::action::{TradeAction, ACTION_DIM};
use crate::agent::DQNAgent;
use crate::network::QNetwork;
use crate::state::{STATE_DIM, STATE_FEATURES};
use protobuf::{f32_raw_data, Message};

/// ONNX IR version 7, which pairs with opset 13.
///
/// Deliberately not the newest. Opset 13 keeps `ReduceMean`'s `axes` as an *attribute*;
/// from opset 18 it became a second *input*, so a graph written the modern way fails to
/// load on older runtimes. Opset 13 loads everywhere current, including the runtimes
/// most likely to be sitting in an existing deployment.
pub const IR_VERSION: i64 = 7;
/// Default ONNX operator set.
pub const OPSET_VERSION: i64 = 13;

/// `TensorProto.DataType.FLOAT`.
const DTYPE_FLOAT: i64 = 1;

/// `AttributeProto.AttributeType`.
const ATTR_INT: i64 = 2;
const ATTR_INTS: i64 = 7;

/// Name of the graph input.
pub const INPUT_NAME: &str = "state";
/// Name of the graph output: Q-values, one per action.
pub const OUTPUT_NAME: &str = "q_values";
/// Dueling only: the state-value scalar, exported as a second output.
pub const VALUE_OUTPUT_NAME: &str = "state_value";

/// Knobs for an export.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub producer_name: String,
    pub producer_version: String,
    pub model_version: i64,
    pub doc_string: String,
    /// Extra `metadata_props` entries written into the model.
    pub metadata: Vec<(String, String)>,
    /// Emit the dueling `V(s)` head as a second output. Free — it is already computed —
    /// and it makes a collapsed value head visible to anything inspecting the model.
    pub include_value_output: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            producer_name: "scematica-nn".to_string(),
            producer_version: env!("CARGO_PKG_VERSION").to_string(),
            model_version: 1,
            doc_string: String::new(),
            metadata: Vec::new(),
            include_value_output: true,
        }
    }
}

impl ExportOptions {
    pub fn with_metadata(mut self, key: &str, value: impl Into<String>) -> Self {
        self.metadata.push((key.to_string(), value.into()));
        self
    }
}

// ── proto builders ───────────────────────────────────────────────────────────────

/// `TensorProto` holding an f32 matrix or vector.
fn tensor(name: &str, dims: &[i64], values: &[f64]) -> Message {
    let mut t = Message::new();
    t.field_packed_i64(1, dims); // dims
    t.field_i64_always(2, DTYPE_FLOAT); // data_type
    t.field_str(8, name); // name
    t.field_bytes(9, &f32_raw_data(values)); // raw_data
    t
}

/// `ValueInfoProto` for a 2-D tensor with a symbolic batch dimension.
///
/// The batch dim is a *name*, not a number, so the exported model accepts any batch size
/// — one state for a live decision, ten thousand for an offline sweep.
fn value_info(name: &str, batch_dim: &str, features: i64) -> Message {
    let mut batch = Message::new();
    batch.field_str(2, batch_dim); // dim_param

    let mut feature = Message::new();
    feature.field_i64_always(1, features); // dim_value

    let mut shape = Message::new();
    shape.field_msg_always(1, &batch);
    shape.field_msg_always(1, &feature);

    let mut tensor_type = Message::new();
    tensor_type.field_i64_always(1, DTYPE_FLOAT); // elem_type
    tensor_type.field_msg_always(2, &shape);

    let mut type_proto = Message::new();
    type_proto.field_msg_always(1, &tensor_type); // TypeProto.tensor_type

    let mut info = Message::new();
    info.field_str(1, name);
    info.field_msg_always(2, &type_proto);
    info
}

/// One `NodeProto`.
fn node(op_type: &str, name: &str, inputs: &[&str], outputs: &[&str], attrs: &[Message]) -> Message {
    let mut n = Message::new();
    for input in inputs {
        n.field_str_always(1, input);
    }
    for output in outputs {
        n.field_str_always(2, output);
    }
    n.field_str(3, name);
    n.field_str(4, op_type);
    for attr in attrs {
        n.field_msg(5, attr);
    }
    n
}

fn attr_int(name: &str, value: i64) -> Message {
    let mut a = Message::new();
    a.field_str(1, name);
    a.field_i64_always(3, value); // i
    a.field_i64_always(20, ATTR_INT); // type
    a
}

fn attr_ints(name: &str, values: &[i64]) -> Message {
    let mut a = Message::new();
    a.field_str(1, name);
    a.field_packed_i64(8, values); // ints
    a.field_i64_always(20, ATTR_INTS); // type
    a
}

fn metadata_entry(key: &str, value: &str) -> Message {
    let mut e = Message::new();
    e.field_str(1, key);
    e.field_str(2, value);
    e
}

/// Flatten `weights[out][in]` into the row-major buffer ONNX expects for `[out, in]`.
fn flatten(weights: &[Vec<f64>]) -> Vec<f64> {
    weights.iter().flat_map(|row| row.iter().copied()).collect()
}

// ── the graph ────────────────────────────────────────────────────────────────────

struct GraphParts {
    nodes: Vec<Message>,
    initializers: Vec<Message>,
    outputs: Vec<Message>,
}

/// Emit a `Gemm` + its weight/bias initialisers, returning the output tensor name.
fn emit_linear(
    parts: &mut GraphParts,
    prefix: &str,
    input: &str,
    weights: &[Vec<f64>],
    biases: &[f64],
) -> String {
    let out_size = weights.len() as i64;
    let in_size = weights.first().map(|row| row.len()).unwrap_or(0) as i64;

    let w_name = format!("{prefix}.weight");
    let b_name = format!("{prefix}.bias");
    let y_name = format!("{prefix}.out");

    // [out, in] with transB=1 — the layout the Rust struct already uses.
    parts
        .initializers
        .push(tensor(&w_name, &[out_size, in_size], &flatten(weights)));
    parts.initializers.push(tensor(&b_name, &[out_size], biases));
    parts.nodes.push(node(
        "Gemm",
        prefix,
        &[input, &w_name, &b_name],
        &[&y_name],
        &[attr_int("transB", 1)],
    ));
    y_name
}

fn build_graph(net: &QNetwork, options: &ExportOptions) -> Message {
    let mut parts = GraphParts {
        nodes: Vec::new(),
        initializers: Vec::new(),
        outputs: Vec::new(),
    };

    let is_dueling = net.value_head.is_some() && net.advantage_head.is_some();
    let last_trunk = net.layers.len().saturating_sub(1);

    let mut cursor = INPUT_NAME.to_string();
    for (index, layer) in net.layers.iter().enumerate() {
        let gemm = emit_linear(
            &mut parts,
            &format!("trunk{index}"),
            &cursor,
            &layer.weights,
            &layer.biases,
        );
        // Dueling: every trunk layer is followed by ReLU, because the outputs come from
        // the heads. Standard: the final layer stays linear — Q-values are unbounded and
        // a ReLU there would clamp every negative Q to zero.
        if is_dueling || index < last_trunk {
            let relu_out = format!("trunk{index}.relu");
            parts.nodes.push(node(
                "Relu",
                &format!("trunk{index}_relu"),
                &[&gemm],
                &[&relu_out],
                &[],
            ));
            cursor = relu_out;
        } else {
            cursor = gemm;
        }
    }

    let action_dim = if is_dueling {
        net.advantage_head.as_ref().map(|h| h.weights.len()).unwrap_or(ACTION_DIM) as i64
    } else {
        net.layers.last().map(|l| l.weights.len()).unwrap_or(ACTION_DIM) as i64
    };

    if is_dueling {
        let value_head = net.value_head.as_ref().expect("checked above");
        let advantage_head = net.advantage_head.as_ref().expect("checked above");

        let v = emit_linear(&mut parts, "value_head", &cursor, &value_head.weights, &value_head.biases);
        let a = emit_linear(
            &mut parts,
            "advantage_head",
            &cursor,
            &advantage_head.weights,
            &advantage_head.biases,
        );

        // Q(s,a) = V(s) + A(s,a) − mean_a A(s,a). keepdims=1 so the mean broadcasts back
        // across the action axis; with keepdims=0 the Sub would broadcast along the
        // wrong dimension and silently produce garbage for batches.
        parts.nodes.push(node(
            "ReduceMean",
            "advantage_mean",
            &[&a],
            &["advantage.mean"],
            &[attr_ints("axes", &[1]), attr_int("keepdims", 1)],
        ));
        parts.nodes.push(node(
            "Sub",
            "advantage_centered",
            &[&a, "advantage.mean"],
            &["advantage.centered"],
            &[],
        ));
        parts.nodes.push(node(
            "Add",
            "q_values_add",
            &["advantage.centered", &v],
            &[OUTPUT_NAME],
            &[],
        ));

        parts.outputs.push(value_info(OUTPUT_NAME, "batch", action_dim));
        if options.include_value_output {
            // Identity rather than renaming the Gemm output: a tensor cannot be both an
            // internal edge and a graph output under a different name.
            parts.nodes.push(node(
                "Identity",
                "state_value_out",
                &[&v],
                &[VALUE_OUTPUT_NAME],
                &[],
            ));
            parts.outputs.push(value_info(VALUE_OUTPUT_NAME, "batch", 1));
        }
    } else {
        parts
            .nodes
            .push(node("Identity", "q_values_out", &[&cursor], &[OUTPUT_NAME], &[]));
        parts.outputs.push(value_info(OUTPUT_NAME, "batch", action_dim));
    }

    let input_dim = net
        .layers
        .first()
        .and_then(|l| l.weights.first())
        .map(|row| row.len())
        .unwrap_or(STATE_DIM) as i64;

    let mut graph = Message::new();
    for n in &parts.nodes {
        graph.field_msg(1, n);
    }
    graph.field_str(2, "scematica_dqn");
    for init in &parts.initializers {
        graph.field_msg(5, init);
    }
    graph.field_str(
        10,
        "Deep Q* action-value network exported from scematica-nn. \
         Input is the normalised 24-feature trade state; output is one Q-value per action.",
    );
    graph.field_msg(11, &value_info(INPUT_NAME, "batch", input_dim));
    for out in &parts.outputs {
        graph.field_msg(12, out);
    }
    graph
}

/// Serialise a [`QNetwork`] as an ONNX `ModelProto`.
pub fn qnetwork_to_onnx(net: &QNetwork, options: &ExportOptions) -> Vec<u8> {
    let graph = build_graph(net, options);

    let mut opset = Message::new();
    opset.field_str(1, ""); // default (ai.onnx) domain
    opset.field_i64_always(2, OPSET_VERSION);

    let mut model = Message::new();
    model.field_i64_always(1, IR_VERSION);
    model.field_str(2, &options.producer_name);
    model.field_str(3, &options.producer_version);
    model.field_str(4, ""); // domain
    model.field_i64(5, options.model_version);
    model.field_str(6, &options.doc_string);
    model.field_msg(7, &graph);
    model.field_msg(8, &opset);

    for (key, value) in &options.metadata {
        model.field_msg(14, &metadata_entry(key, value));
    }

    model.into_bytes()
}

/// Write a [`QNetwork`] to `path` as ONNX.
pub fn export_qnetwork(net: &QNetwork, path: &str, options: &ExportOptions) -> std::io::Result<()> {
    let bytes = qnetwork_to_onnx(net, options);
    // Same atomic-rename convention as the rest of the project's file writers: a reader
    // never sees a half-written model.
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Standard metadata describing what the model is and what it was trained on.
///
/// Feature and action names matter more than they look: a bare `[batch, 24]` input is
/// unusable by anyone who does not have this source open, and a consumer that mis-orders
/// two features gets a confidently wrong policy with no error. Writing the schema into
/// the file makes the model self-describing.
pub fn describe(net: &QNetwork) -> Vec<(String, String)> {
    let is_dueling = net.value_head.is_some() && net.advantage_head.is_some();
    let actions: Vec<&str> = (0..ACTION_DIM)
        .map(|i| TradeAction::from_index(i).label())
        .collect();

    vec![
        ("framework".into(), "scematica-nn".into()),
        (
            "architecture".into(),
            if is_dueling {
                "dueling-double-dqn".into()
            } else {
                "double-dqn".into()
            },
        ),
        ("state_dim".into(), STATE_DIM.to_string()),
        ("action_dim".into(), ACTION_DIM.to_string()),
        ("action_labels".into(), actions.join(",")),
        ("state_features".into(), STATE_FEATURES.join(",")),
        (
            "input_normalisation".into(),
            "all features pre-normalised to [0,1] by TradeState::to_vec".into(),
        ),
        (
            "layer_sizes".into(),
            net.layer_sizes
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("x"),
        ),
    ]
}

/// Export the agent's **online** network — the one that selects actions — with the
/// training state that produced it recorded alongside.
///
/// The training counters are not decoration. A Q-network's weights say nothing about
/// whether they are worth anything; `train_steps` and `epsilon` are how a consumer knows
/// whether this is a converged policy or a barely-trained one still exploring. The
/// crate's own gate for acting on the network is 10,000 train steps, and that threshold
/// travels with the file.
pub fn export_agent(agent: &DQNAgent, path: &str) -> std::io::Result<()> {
    let net = agent.online_net();
    let stats = agent.stats();

    let mut options = ExportOptions {
        doc_string: format!(
            "Scematica Deep Q* policy network. Trained {} steps, epsilon {:.4}, \
             total reward {:.4}. Q-values are per-action expected returns; \
             argmax is the greedy policy.",
            stats.train_steps, stats.epsilon, stats.total_reward
        ),
        ..Default::default()
    };
    options.metadata = describe(net);
    options.metadata.push(("train_steps".into(), stats.train_steps.to_string()));
    options.metadata.push(("step_count".into(), stats.step_count.to_string()));
    options.metadata.push(("epsilon".into(), format!("{:.6}", stats.epsilon)));
    options
        .metadata
        .push(("total_reward".into(), format!("{:.6}", stats.total_reward)));
    options
        .metadata
        .push(("target_updates".into(), stats.target_updates.to_string()));
    options
        .metadata
        .push(("ready_to_advise".into(), stats.ready_to_advise.to_string()));
    options
        .metadata
        .push(("replay_size".into(), stats.replay_size.to_string()));
    options
        .metadata
        .push(("exported_at".into(), chrono::Utc::now().to_rfc3339()));

    export_qnetwork(net, path, &options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::QNetwork;

    fn dueling() -> QNetwork {
        QNetwork::new_dueling(&[STATE_DIM, 8, 4], ACTION_DIM)
    }

    #[test]
    fn export_produces_a_non_trivial_model() {
        let bytes = qnetwork_to_onnx(&dueling(), &ExportOptions::default());
        assert!(bytes.len() > 512, "model suspiciously small: {}", bytes.len());
    }

    #[test]
    fn model_starts_with_ir_version_field() {
        // Field 1, wire type 0 => key 0x08, then the IR version varint.
        let bytes = qnetwork_to_onnx(&dueling(), &ExportOptions::default());
        assert_eq!(bytes[0], 0x08);
        assert_eq!(bytes[1], IR_VERSION as u8);
    }

    #[test]
    fn producer_name_is_embedded() {
        let bytes = qnetwork_to_onnx(&dueling(), &ExportOptions::default());
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(haystack.contains("scematica-nn"));
        assert!(haystack.contains(OUTPUT_NAME));
        assert!(haystack.contains(INPUT_NAME));
    }

    #[test]
    fn dueling_export_contains_the_recombination_ops() {
        let bytes = qnetwork_to_onnx(&dueling(), &ExportOptions::default());
        let haystack = String::from_utf8_lossy(&bytes);
        for op in ["Gemm", "Relu", "ReduceMean", "Sub", "Add"] {
            assert!(haystack.contains(op), "missing op {op}");
        }
    }

    #[test]
    fn standard_export_has_no_dueling_ops() {
        let net = QNetwork::new(&[STATE_DIM, 8, ACTION_DIM]);
        let bytes = qnetwork_to_onnx(&net, &ExportOptions::default());
        let haystack = String::from_utf8_lossy(&bytes);
        assert!(haystack.contains("Gemm"));
        assert!(!haystack.contains("ReduceMean"));
    }

    #[test]
    fn metadata_describes_the_io_schema() {
        let net = dueling();
        let describe_pairs = describe(&net);
        let keys: Vec<&str> = describe_pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"state_features"));
        assert!(keys.contains(&"action_labels"));

        let features = describe_pairs
            .iter()
            .find(|(k, _)| k == "state_features")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(features.split(',').count(), STATE_DIM);
    }

    #[test]
    fn weight_count_matches_the_network() {
        // Every parameter must reach the file: trunk layers plus both heads.
        let net = dueling();
        let params: usize = net
            .layers
            .iter()
            .map(|l| l.weights.len() * l.weights[0].len() + l.biases.len())
            .sum::<usize>()
            + net
                .value_head
                .as_ref()
                .map(|h| h.weights.len() * h.weights[0].len() + h.biases.len())
                .unwrap_or(0)
            + net
                .advantage_head
                .as_ref()
                .map(|h| h.weights.len() * h.weights[0].len() + h.biases.len())
                .unwrap_or(0);

        let bytes = qnetwork_to_onnx(&net, &ExportOptions::default());
        // Every parameter is 4 bytes of raw_data; the file must be at least that big.
        assert!(bytes.len() > params * 4);
    }
}
