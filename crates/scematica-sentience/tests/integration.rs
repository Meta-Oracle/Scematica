use scematica_sentience::{
    CognitiveLoop, CognitiveState,
    agency::AgencyInputs,
    ethics::{ActionEvaluation, EthicsInputs},
    growth_model::GrowthModel,
    logic::LogicInputs,
    master_equation::MasterEquation,
    meta_cognition::MetaCognitionInputs,
    perception::Perception,
    rationality::RationalityInputs,
    sentience::SentienceIndex,
    types::{Bounded, Observation},
};

// ── §1: Sentience = 0 when any component = 0 ─────────────────────────────────
#[test]
fn sentience_zero_when_data_zero() {
    let r = RationalityInputs::default();
    let l = LogicInputs::default();
    let m = EthicsInputs::default();
    let d = Perception::new(0.0, 1.0, 1.0, 1.0); // audio = 0
    let s = SentienceIndex::compute(&r, &l, &m, &d);
    assert_eq!(s.value.value(), 0.0, "S must be 0 when any perception channel is 0");
}

#[test]
fn sentience_zero_when_ethics_zero() {
    let r = RationalityInputs::default();
    let l = LogicInputs::default();
    let m = EthicsInputs::new(0.0, 1.0, 1.0, 1.0); // harm_minimization = 0
    let d = Perception::default();
    let s = SentienceIndex::compute(&r, &l, &m, &d);
    assert_eq!(s.value.value(), 0.0);
}

#[test]
fn sentience_bounded_to_one() {
    let r = RationalityInputs::default();
    let l = LogicInputs::default();
    let m = EthicsInputs::default();
    let d = Perception::default();
    let s = SentienceIndex::compute(&r, &l, &m, &d);
    assert!(s.value.value() <= 1.0);
    assert!(s.value.value() > 0.0);
}

// ── §3: Rationality ─────────────────────────────────────────────────────────
#[test]
fn rationality_high_bias_reduces_score() {
    let low_bias  = RationalityInputs::new(0.9, 0.9, 0.8, 0.01);
    let high_bias = RationalityInputs::new(0.9, 0.9, 0.8, 0.9);
    assert!(low_bias.rationality().value() > high_bias.rationality().value());
}

// ── §5: Ethics hard gate ─────────────────────────────────────────────────────
#[test]
fn ethics_gate_blocks_action() {
    let action = ActionEvaluation {
        action_id: "test".into(),
        expected_benefit: 100.0,
        expected_harm: 1.0,
        risk: 0.5,
        constraints_satisfied: false, // hard gate fails
        safety_verified: true,
        system_constraints_satisfied: true,
    };
    assert!(action.ethical_utility().is_none(), "P(a)=0 when constraint fails");
}

#[test]
fn ethics_gate_permits_good_action() {
    let action = ActionEvaluation {
        action_id: "good".into(),
        expected_benefit: 10.0,
        expected_harm: 1.0,
        risk: 0.5,
        constraints_satisfied: true,
        safety_verified: true,
        system_constraints_satisfied: true,
    };
    let u = action.ethical_utility().expect("permitted action should have utility");
    assert!((u - 8.5).abs() < 1e-9);
}

// ── §26: Growth model saturates ──────────────────────────────────────────────
#[test]
fn growth_model_saturates_at_c_max() {
    let mut gm = GrowthModel::new(1.0, 10.0, 0.5);
    for _ in 0..200 {
        gm.step(1.0, 1.0, 1.0);
    }
    assert!(gm.capability <= gm.c_max, "capability must not exceed c_max");
    assert!(gm.utilization() > 0.99, "should saturate near ceiling after 200 steps");
}

#[test]
fn growth_model_zero_inputs_no_growth() {
    let mut gm = GrowthModel::new(1.0, 10.0, 0.5);
    let before = gm.capability;
    gm.step(0.0, 0.0, 0.0);
    // With zero growth factor exp(-0)=1 so ratio stays same → capability unchanged
    assert!((gm.capability - before).abs() < 1e-9);
}

// ── §23/§27: Master equation ──────────────────────────────────────────────────
#[test]
fn master_equation_psi_bounded() {
    let (_, psi) = MasterEquation::compute(
        &RationalityInputs::default(),
        &LogicInputs::default(),
        &EthicsInputs::default(),
        &Perception::default(),
        &AgencyInputs::default(),
        &MetaCognitionInputs::default(),
        Bounded::new(0.7),
        Bounded::new(0.9),
    );
    assert!(psi.psi.value() <= 1.0);
    assert!(psi.psi.value() > 0.0);
}

// ── §24: Cognitive loop advances timestep ────────────────────────────────────
#[test]
fn cognitive_loop_advances_timestep() {
    let state = CognitiveState::initial();
    let mut engine = CognitiveLoop::new(state);
    for i in 1..=5 {
        let obs = Observation { value: 0.7, confidence: 0.9.into(), provenance: None, timestep: i };
        let out = engine.step(obs, 0.5, 0.9);
        assert_eq!(out.timestep, i as u64);
    }
}

// ── §2: Data integrity geometric mean ────────────────────────────────────────
#[test]
fn data_integrity_geometric_mean() {
    use scematica_sentience::data_integrity::DataIntegrityInputs;
    let di = DataIntegrityInputs::new(1.0, 1.0, 1.0, 1.0);
    assert!((di.integrity().value() - 1.0).abs() < 1e-9);

    let di2 = DataIntegrityInputs::new(0.0, 1.0, 1.0, 1.0);
    assert_eq!(di2.integrity().value(), 0.0);
}
