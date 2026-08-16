//! Pure view logic: where each node sits on a character grid, and what tone it earns.
//!
//! Everything here is a pure function of a [`Mesh`]. No terminal, no ratatui, no I/O — so
//! the layout arithmetic and, more importantly, the *tone rules* are testable with
//! `cargo test` and cannot drift per widget.
//!
//! This is the terminal counterpart of `web/lib/mesh/view.ts`, and the two must agree on
//! the rules even though they disagree on units (SVG pixels there, character cells here).
//! The parts that must match exactly are the layer assignment — which comes from
//! [`NodeKind::layer`] in `scematica-mesh`, authoritative for both — and the tone rule
//! below.
//!
//! ## THE TONE RULE, which is the part worth protecting
//!
//! Tone is a claim about how much the reader may trust a number, and it is assigned in one
//! place — [`tone_for`] — so it cannot drift.
//!
//! ```text
//! live      the unit is reporting now; its numbers are actionable
//! stale     the unit reported once and has gone quiet; numbers are history
//! absent    the unit cannot be seen at all; there are no numbers
//! veto      this unit is actively stopping the system
//! ```
//!
//! Provenance outranks verdict for everything except an active veto from a live source. A
//! stale node reading PASS has not passed anything recently, and painting it the same
//! green as a live pass is the exact error this whole tool exists to prevent.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use scematica_mesh::{Edge, EdgeKind, Mesh, Node, Provenance, Verdict};

/// Column headings, indexed by [`scematica_mesh::NodeKind::layer`].
pub const LAYER_TITLES: [&str; 6] =
    ["Ingest", "Filter", "Risk", "Cognition", "Execution", "Mesh"];

/// The semantic tones. Render code asks for a tone and never for a colour, so a palette
/// change happens in exactly one place (`render::colour_of`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Live,
    Stale,
    Absent,
    Veto,
    Simulated,
}

/// The tone a node earns.
///
/// An active veto is the only condition allowed to override provenance, and only from a
/// live source: a veto recovered from a three-month-old file is history, and painting it
/// alarm-red sends an operator hunting a gate that may have opened long ago.
pub fn tone_for(node: &Node) -> Tone {
    if node.verdict == Verdict::Veto && matches!(node.provenance, Provenance::Live { .. }) {
        return Tone::Veto;
    }
    match node.provenance {
        Provenance::Live { .. } => Tone::Live,
        Provenance::Stale { .. } => Tone::Stale,
        Provenance::Absent => Tone::Absent,
        Provenance::Simulated => Tone::Simulated,
    }
}

/// Coarse human duration. Deliberately coarse, matching `topology::humanise` in the
/// collector: a veto that has stood for "3h" and one that has stood for "3h 14m" call for
/// the same action, and the extra precision only invites the reader to treat a staleness
/// figure as a measurement.
pub fn humanise(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3_600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// Human age for a provenance, or `None` when there is nothing to age.
pub fn age_label(p: &Provenance) -> Option<String> {
    match p {
        Provenance::Live { age_secs } => Some(humanise(*age_secs)),
        Provenance::Stale { age_secs, .. } => Some(humanise(*age_secs)),
        Provenance::Absent | Provenance::Simulated => None,
    }
}

/// The word for a provenance, used wherever tone alone would be the only signal.
///
/// Colour is decoration, never the message: piped output, a 16-colour terminal and
/// `NO_COLOR` must all still say which nodes can be believed.
pub fn provenance_word(p: &Provenance) -> &'static str {
    match p {
        Provenance::Live { .. } => "live",
        Provenance::Stale { .. } => "STALE",
        Provenance::Absent => "unseen",
        Provenance::Simulated => "sim",
    }
}

pub fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "pass",
        Verdict::Veto => "VETO",
        Verdict::Damp => "damp",
        Verdict::Degraded => "degraded",
        Verdict::Idle => "idle",
        Verdict::Unknown => "unknown",
    }
}

/// Is this edge actively stopping flow?
///
/// Tri-state on purpose. `None` (unreadable) must reach the renderer intact so it draws a
/// dashed "unknown" gate rather than an open one — an unexamined veto is not a cleared
/// veto, the same rule `Edge::is_blocking` follows in the collector.
///
/// Takes the two fields rather than an `&Edge` so the renderer can ask the same question
/// of a [`PlacedEdge`]. One definition of the rule, reachable from both sides of layout —
/// a second copy in the renderer is how a cleared veto and an unreadable one end up drawn
/// the same.
pub fn edge_blocking(kind: EdgeKind, active: Option<bool>) -> Option<bool> {
    if kind != EdgeKind::Veto {
        return Some(false);
    }
    active
}

/// Stable identity for an edge, used by trace sets and lane packing alike.
pub fn edge_key(edge: &Edge) -> String {
    format!("{}->{}:{:?}", edge.from, edge.to, edge.kind)
}

/// The single line that belongs above the graph.
pub fn visibility_label(mesh: &Mesh) -> String {
    let s = &mesh.summary;
    format!(
        "{:.0}% visible · {} live · {} stale · {} unseen",
        s.visibility * 100.0,
        s.nodes_live,
        s.nodes_stale,
        s.nodes_absent
    )
}

/// Every node that can reach `id`, and every node `id` can reach.
///
/// This is the "why did nothing trade" question asked structurally: select the Executor
/// and the trace is exactly the set of units with any say in that outcome, with everything
/// irrelevant dimmed away. On a 22-node graph the difference between *seeing* the topology
/// and *following* it is the difference between a diagram and a tool.
///
/// Breadth-first over both directions, guarded against cycles — the graph has feedback
/// edges (promotion runs backwards into the primary learner) and an unguarded walk would
/// not terminate.
pub struct Trace {
    pub nodes: HashSet<String>,
    pub edges: HashSet<String>,
}

pub fn trace(mesh: &Mesh, id: &str) -> Trace {
    let mut nodes: HashSet<String> = HashSet::new();
    let mut edges: HashSet<String> = HashSet::new();
    nodes.insert(id.to_string());

    for forward in [true, false] {
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        seen.insert(id.to_string());
        queue.push_back(id.to_string());

        while let Some(cur) = queue.pop_front() {
            for e in &mesh.edges {
                let (from, to) = if forward { (&e.from, &e.to) } else { (&e.to, &e.from) };
                if *from != cur {
                    continue;
                }
                edges.insert(edge_key(e));
                nodes.insert(to.clone());
                if seen.insert(to.clone()) {
                    queue.push_back(to.clone());
                }
            }
        }
    }

    Trace { nodes, edges }
}

// ── layout ───────────────────────────────────────────────────────────────────

/// Width of a node box in character cells, including its borders.
pub const NODE_W: u16 = 24;
/// Height of a node box in character cells, including its borders.
pub const NODE_H: u16 = 5;
/// Blank rows between stacked nodes in one column.
pub const ROW_GAP: u16 = 1;
/// Width of the routing channel between columns. Must be ≥ 3 so an edge has at least one
/// interior column to turn in without touching either node box.
pub const GUTTER: u16 = 7;
/// Rows reserved at the top for column headings.
pub const HEADER_ROWS: u16 = 2;

#[derive(Clone, Debug)]
pub struct Placed {
    pub id: String,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// Index into [`LAYER_TITLES`].
    pub layer: u8,
}

impl Placed {
    /// Vertical centre row — where an edge attaches.
    pub fn cy(&self) -> u16 {
        self.y + self.h / 2
    }
    pub fn right(&self) -> u16 {
        self.x + self.w
    }
}

#[derive(Clone, Debug)]
pub struct Column {
    pub x: u16,
    /// Resolved from the Rust layer table once, here. The renderer prints this rather
    /// than looking the title up a second time from a layer index it carries separately.
    pub title: &'static str,
}

/// How an edge gets from its source to its target.
///
/// Two shapes, because the graph is not a clean chain. `Direct` is the common adjacent
/// hop drawn in the gutter between two columns. `Lane` is everything else — a same-column
/// feedback edge (tournament promotion), or one that skips a column entirely (every risk
/// breaker sits in Risk and vetoes Execution, jumping straight over Cognition). Drawing
/// those in a gutter would run a line straight through the boxes in between, so they drop
/// to a horizontal lane below the graph and come back up.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Direct {
        /// x of the vertical turn inside the gutter.
        channel: u16,
    },
    Lane {
        /// x of the vertical drop out of the source.
        down_x: u16,
        /// x of the vertical climb into the target.
        up_x: u16,
        /// Absolute row the horizontal run uses.
        lane_y: u16,
    },
}

#[derive(Clone, Debug)]
pub struct PlacedEdge {
    pub key: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub active: Option<bool>,
    pub label: Option<String>,
    pub route: Route,
    /// Attachment points, precomputed so the canvas never re-derives geometry.
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

#[derive(Clone, Debug, Default)]
pub struct GraphLayout {
    pub placed: Vec<Placed>,
    pub edges: Vec<PlacedEdge>,
    pub columns: Vec<Column>,
    pub width: u16,
    pub height: u16,
}

impl GraphLayout {
    pub fn find(&self, id: &str) -> Option<&Placed> {
        self.placed.iter().find(|p| p.id == id)
    }
}

/// Place nodes in columns by layer, stacked and vertically centred within each column.
///
/// Deterministic and order-stable: nodes keep the order the collector emitted them in, so
/// a node does not jump rows between polls when an unrelated one appears. A layout that
/// reshuffles on every refresh is unreadable no matter how good it looks in a screenshot.
pub fn layout(mesh: &Mesh) -> GraphLayout {
    // BTreeMap so layers come out ordered without a separate sort, and so the iteration
    // order is stable across runs.
    let mut by_layer: BTreeMap<u8, Vec<&Node>> = BTreeMap::new();
    for n in &mesh.nodes {
        by_layer.entry(n.kind.layer()).or_default().push(n);
    }

    let tallest = by_layer.values().map(|v| v.len()).max().unwrap_or(0) as u16;
    let col_span = |count: u16| -> u16 {
        if count == 0 {
            0
        } else {
            count * (NODE_H + ROW_GAP) - ROW_GAP
        }
    };
    let tallest_span = col_span(tallest);

    let mut placed = Vec::new();
    let mut columns = Vec::new();
    let mut pos: HashMap<String, Placed> = HashMap::new();

    for (col_index, (layer, nodes)) in by_layer.iter().enumerate() {
        // A leading gutter, so column 0 has somewhere to route a feedback edge into.
        let x = GUTTER + col_index as u16 * (NODE_W + GUTTER);
        let span = col_span(nodes.len() as u16);
        let top = HEADER_ROWS + (tallest_span - span) / 2;

        columns.push(Column {
            x,
            title: LAYER_TITLES.get(*layer as usize).copied().unwrap_or("—"),
        });

        for (i, n) in nodes.iter().enumerate() {
            let p = Placed {
                id: n.id.clone(),
                x,
                y: top + i as u16 * (NODE_H + ROW_GAP),
                w: NODE_W,
                h: NODE_H,
                layer: *layer,
            };
            pos.insert(n.id.clone(), p.clone());
            placed.push(p);
        }
    }

    let graph_bottom = HEADER_ROWS + tallest_span;
    let width = GUTTER + by_layer.len() as u16 * (NODE_W + GUTTER);

    // Channel allocation, per gutter. Parallel edges sharing one gutter must turn at
    // different x or they overlap into a single indistinguishable line.
    let mut channel_use: HashMap<u16, u16> = HashMap::new();
    // Lane packing: an occupied x-interval per lane row. A lane is reused whenever the new
    // edge's horizontal run does not overlap anything already on it, which collapses ~15
    // lane edges into a handful of rows instead of 15.
    let mut lanes: Vec<Vec<(u16, u16)>> = Vec::new();

    let mut edges = Vec::new();
    for e in &mesh.edges {
        // A dangling edge is dropped rather than drawn into empty space. `Mesh::validate`
        // is what should have caught it; drawing a line to (0,0) would make a broken
        // topology look like a real connection.
        let (Some(a), Some(b)) = (pos.get(&e.from), pos.get(&e.to)) else { continue };

        let adjacent_forward = b.x == a.right() + GUTTER;
        let route = if adjacent_forward {
            let gutter_start = a.right();
            let k = channel_use.entry(gutter_start).or_insert(0);
            // Interior of the gutter only: never the cell flush against either box.
            let channel = gutter_start + 1 + (*k % (GUTTER.saturating_sub(2)).max(1));
            *k += 1;
            Route::Direct { channel }
        } else {
            let down_x = a.right() + 1;
            // Climb back up in the gutter immediately left of the target.
            let up_x = b.x.saturating_sub(GUTTER) + GUTTER / 2;
            let (lo, hi) = (down_x.min(up_x), down_x.max(up_x));
            // Two passes, and the first one matters more than it looks. Edges leaving one
            // column for one target share a span exactly — seven filters reporting to the
            // pipeline, six breakers vetoing the executor. Their lines are *identical*, so
            // packing them onto separate rows draws a seven-rung ladder of duplicates
            // instead of the bus they actually are. Reuse the row when the span already
            // exists; only fall back to interval packing for genuinely different spans.
            let lane_index = lanes
                .iter()
                .position(|occupied| occupied.contains(&(lo, hi)))
                .or_else(|| {
                    lanes
                        .iter()
                        .position(|occupied| occupied.iter().all(|(s, t)| hi < *s || lo > *t))
                })
                .unwrap_or_else(|| {
                    lanes.push(Vec::new());
                    lanes.len() - 1
                });
            lanes[lane_index].push((lo, hi));
            Route::Lane { down_x, up_x, lane_y: graph_bottom + 1 + lane_index as u16 }
        };

        edges.push(PlacedEdge {
            key: edge_key(e),
            from: e.from.clone(),
            to: e.to.clone(),
            kind: e.kind,
            active: e.active,
            label: e.label.clone(),
            route,
            x1: a.right(),
            y1: a.cy(),
            x2: b.x.saturating_sub(1),
            y2: b.cy(),
        });
    }

    let height = graph_bottom + 1 + lanes.len() as u16 + 1;

    GraphLayout { placed, edges, columns, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scematica_mesh::{Edge, NodeKind};

    fn node(id: &str, kind: NodeKind, provenance: Provenance, verdict: Verdict) -> Node {
        Node {
            id: id.into(),
            kind,
            label: id.into(),
            blurb: String::new(),
            provenance,
            verdict,
            activity: None,
            detail: vec![],
            reason: None,
        }
    }

    const LIVE: Provenance = Provenance::Live { age_secs: 3 };
    const STALE: Provenance = Provenance::Stale { age_secs: 900_000, budget_secs: 30 };

    // ── the tone rule ────────────────────────────────────────────────────────

    /// The single most important assertion in this file. A stale node reading PASS is
    /// history, and green would tell the operator to act on a three-month-old number.
    #[test]
    fn provenance_outranks_verdict() {
        let n = node("a", NodeKind::Filter, STALE, Verdict::Pass);
        assert_eq!(tone_for(&n), Tone::Stale);
    }

    #[test]
    fn a_live_veto_is_the_one_thing_that_overrides_provenance() {
        let n = node("a", NodeKind::Learner, LIVE, Verdict::Veto);
        assert_eq!(tone_for(&n), Tone::Veto);
    }

    /// A veto recovered from a cold file is history, not an alarm. Painting it red sends
    /// an operator hunting a gate that may have opened months ago.
    #[test]
    fn a_stale_veto_is_not_painted_as_an_alarm() {
        let n = node("a", NodeKind::Learner, STALE, Verdict::Veto);
        assert_eq!(tone_for(&n), Tone::Stale);
    }

    #[test]
    fn an_absent_node_is_never_confused_with_an_idle_one() {
        let n = node("a", NodeKind::Breaker, Provenance::Absent, Verdict::Unknown);
        assert_eq!(tone_for(&n), Tone::Absent);
        assert_eq!(age_label(&Provenance::Absent), None);
    }

    // ── tri-state edges ──────────────────────────────────────────────────────

    /// `None` must survive to the renderer. An unexamined veto is not a cleared veto.
    #[test]
    fn an_unreadable_veto_stays_unreadable() {
        assert_eq!(edge_blocking(EdgeKind::Veto, None), None);
        assert_eq!(edge_blocking(EdgeKind::Veto, Some(false)), Some(false));
        assert_eq!(edge_blocking(EdgeKind::Veto, Some(true)), Some(true));
    }

    #[test]
    fn a_signal_edge_never_reports_as_blocking() {
        assert_eq!(edge_blocking(EdgeKind::Signal, None), Some(false));
        assert_eq!(edge_blocking(EdgeKind::Signal, Some(true)), Some(false));
    }

    // ── layout ───────────────────────────────────────────────────────────────

    fn sample() -> Mesh {
        Mesh::new(
            vec![
                node("l", NodeKind::Listener, LIVE, Verdict::Pass),
                node("f1", NodeKind::Filter, LIVE, Verdict::Pass),
                node("f2", NodeKind::Filter, LIVE, Verdict::Veto),
                node("b1", NodeKind::Breaker, Provenance::Absent, Verdict::Unknown),
                node("d", NodeKind::Learner, LIVE, Verdict::Veto),
                node("x", NodeKind::Executor, LIVE, Verdict::Idle),
            ],
            vec![
                Edge::signal("l", "f1"),
                Edge::signal("f2", "f1"),
                Edge::veto("b1", "x").with_active(Some(true)),
                Edge::signal("d", "x"),
            ],
            "t".into(),
        )
    }

    #[test]
    fn columns_follow_the_rust_layer_table() {
        let g = layout(&sample());
        assert_eq!(g.find("l").unwrap().layer, 0);
        assert_eq!(g.find("f1").unwrap().layer, 1);
        assert_eq!(g.find("b1").unwrap().layer, 2);
        assert_eq!(g.find("d").unwrap().layer, 3);
        assert_eq!(g.find("x").unwrap().layer, 4);
        // Left to right, strictly increasing, one column per layer present.
        let xs: Vec<u16> = g.columns.iter().map(|c| c.x).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "columns must march rightwards: {xs:?}");
    }

    /// Two polls of an unchanged mesh must produce an identical picture. A layout that
    /// reshuffles is unreadable regardless of how good a single frame looks.
    #[test]
    fn layout_is_deterministic() {
        let m = sample();
        let a = layout(&m);
        let b = layout(&m);
        let key = |g: &GraphLayout| {
            g.placed.iter().map(|p| (p.id.clone(), p.x, p.y)).collect::<Vec<_>>()
        };
        assert_eq!(key(&a), key(&b));
        assert_eq!(a.width, b.width);
        assert_eq!(a.height, b.height);
    }

    #[test]
    fn nodes_in_a_column_never_overlap() {
        let g = layout(&sample());
        for a in &g.placed {
            for b in &g.placed {
                if a.id == b.id || a.x != b.x {
                    continue;
                }
                let disjoint = a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(disjoint, "{} and {} overlap in a column", a.id, b.id);
            }
        }
    }

    /// An adjacent hop turns inside the gutter; anything else drops to a lane. Routing a
    /// column-skipping edge through a gutter would draw a line straight through the boxes
    /// it passes — every risk breaker vetoes Execution across Cognition, so this is the
    /// common case, not an exotic one.
    #[test]
    fn a_column_skipping_edge_takes_a_lane_not_a_gutter() {
        let g = layout(&sample());
        let e = g.edges.iter().find(|e| e.from == "b1" && e.to == "x").unwrap();
        assert!(matches!(e.route, Route::Lane { .. }), "breaker→executor skips Cognition");

        let adj = g.edges.iter().find(|e| e.from == "l" && e.to == "f1").unwrap();
        assert!(matches!(adj.route, Route::Direct { .. }), "ingest→filter is adjacent");
    }

    #[test]
    fn a_same_column_feedback_edge_takes_a_lane() {
        let g = layout(&sample());
        let e = g.edges.iter().find(|e| e.from == "f2" && e.to == "f1").unwrap();
        assert!(matches!(e.route, Route::Lane { .. }));
    }

    /// A gutter turn must land strictly inside the gutter. One cell either way and the
    /// line is drawn on top of a node border, which reads as a connection into the box's
    /// edge rather than into its port.
    #[test]
    fn gutter_channels_stay_inside_the_gutter() {
        let g = layout(&sample());
        for e in &g.edges {
            if let Route::Direct { channel } = e.route {
                let a = g.find(&e.from).unwrap();
                assert!(
                    channel > a.right() && channel < a.right() + GUTTER,
                    "channel {channel} escaped the gutter after {}",
                    e.from
                );
            }
        }
    }

    /// Lanes live below the last node row, or they would be drawn over the graph.
    #[test]
    fn lanes_sit_below_every_node() {
        let g = layout(&sample());
        let lowest = g.placed.iter().map(|p| p.y + p.h).max().unwrap();
        for e in &g.edges {
            if let Route::Lane { lane_y, .. } = e.route {
                assert!(lane_y >= lowest, "lane {lane_y} collides with the graph body");
                assert!(lane_y < g.height, "lane {lane_y} escapes the canvas");
            }
        }
    }

    /// Lanes are packed by interval, not allocated one-per-edge. Six breakers vetoing one
    /// executor genuinely need six rows; edges with disjoint spans must share.
    #[test]
    fn disjoint_lane_edges_share_a_row() {
        let m = Mesh::new(
            vec![
                node("a1", NodeKind::Listener, LIVE, Verdict::Pass),
                node("a2", NodeKind::Listener, LIVE, Verdict::Pass),
                node("z1", NodeKind::Executor, LIVE, Verdict::Idle),
                node("z2", NodeKind::Executor, LIVE, Verdict::Idle),
            ],
            // Two same-column feedback edges, in columns far apart: disjoint x-spans.
            vec![Edge::signal("a2", "a1"), Edge::signal("z2", "z1")],
            "t".into(),
        );
        let g = layout(&m);
        let ys: Vec<u16> = g
            .edges
            .iter()
            .filter_map(|e| match e.route {
                Route::Lane { lane_y, .. } => Some(lane_y),
                _ => None,
            })
            .collect();
        assert_eq!(ys.len(), 2);
        assert_eq!(ys[0], ys[1], "disjoint spans must pack onto one lane");
    }

    /// Edges that converge from one column onto one target trace the same three segments.
    /// Giving each its own row draws a ladder of identical rungs — the seven filters
    /// reporting to the pipeline are one bus, and must render as one.
    #[test]
    fn identical_lane_spans_merge_into_one_bus() {
        let mut nodes = vec![node("hub", NodeKind::Filter, LIVE, Verdict::Pass)];
        let mut edges = Vec::new();
        for i in 0..7 {
            let id = format!("f{i}");
            nodes.push(node(&id, NodeKind::Filter, LIVE, Verdict::Veto));
            edges.push(Edge::signal(&id, "hub"));
        }
        let g = layout(&Mesh::new(nodes, edges, "t".into()));
        let rows: HashSet<u16> = g
            .edges
            .iter()
            .filter_map(|e| match e.route {
                Route::Lane { lane_y, .. } => Some(lane_y),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 1, "seven identical spans drew {} rows", rows.len());
    }

    #[test]
    fn parallel_edges_in_one_gutter_get_distinct_channels() {
        let m = Mesh::new(
            vec![
                node("a", NodeKind::Listener, LIVE, Verdict::Pass),
                node("b", NodeKind::Listener, LIVE, Verdict::Pass),
                node("c", NodeKind::Filter, LIVE, Verdict::Pass),
            ],
            vec![Edge::signal("a", "c"), Edge::signal("b", "c")],
            "t".into(),
        );
        let g = layout(&m);
        let chans: Vec<u16> = g
            .edges
            .iter()
            .filter_map(|e| match e.route {
                Route::Direct { channel } => Some(channel),
                _ => None,
            })
            .collect();
        assert_eq!(chans.len(), 2);
        assert_ne!(chans[0], chans[1], "parallel edges must not share a channel");
    }

    #[test]
    fn a_dangling_edge_is_dropped_rather_than_drawn_into_space() {
        let m = Mesh::new(
            vec![node("a", NodeKind::Listener, LIVE, Verdict::Pass)],
            vec![Edge::signal("a", "ghost")],
            "t".into(),
        );
        assert!(layout(&m).edges.is_empty());
    }

    #[test]
    fn an_empty_mesh_lays_out_without_panicking() {
        let g = layout(&Mesh::new(vec![], vec![], "t".into()));
        assert!(g.placed.is_empty());
        assert_eq!(g.columns.len(), 0);
    }

    // ── trace ────────────────────────────────────────────────────────────────

    #[test]
    fn trace_reaches_both_directions() {
        let t = trace(&sample(), "x");
        // Upstream of the executor: the breaker vetoing it and the learner feeding it.
        assert!(t.nodes.contains("b1"));
        assert!(t.nodes.contains("d"));
        assert!(t.nodes.contains("x"));
        // The filter column has no path to the executor in this fixture.
        assert!(!t.nodes.contains("f1"));
    }

    /// The graph has feedback edges (promotion runs backwards into the primary learner).
    /// An unguarded walk would not terminate.
    #[test]
    fn trace_terminates_on_a_cycle() {
        let m = Mesh::new(
            vec![
                node("a", NodeKind::Learner, LIVE, Verdict::Pass),
                node("b", NodeKind::Learner, LIVE, Verdict::Pass),
            ],
            vec![Edge::signal("a", "b"), Edge::signal("b", "a")],
            "t".into(),
        );
        let t = trace(&m, "a");
        assert_eq!(t.nodes.len(), 2);
    }

    #[test]
    fn humanise_matches_the_collectors_own_scale() {
        assert_eq!(humanise(45), "45s");
        assert_eq!(humanise(600), "10m");
        assert_eq!(humanise(7_200), "2h");
        assert_eq!(humanise(7_000_000), "81d");
    }
}
