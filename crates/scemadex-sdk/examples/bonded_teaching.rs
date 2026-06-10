//! Bonded machine teaching (Primitive H): distillation as a metered service
//! with a money-back guarantee. A student streams its most uncertain states to
//! a teacher; every answer costs tuition, and the teacher's bond rides on the
//! student's *measured* improvement.
//!
//!   cargo run -p scemadex-sdk --example bonded_teaching

use scemadex_sdk::{
    BondOutcome, ReferenceTeacher, TeachTerms, TeachingEngine, Usdc,
};

#[tokio::main]
async fn main() -> scemadex_sdk::Result<()> {
    let engine = TeachingEngine::new();
    let teacher = ReferenceTeacher;

    // The student measures its baseline on a holdout set; the teacher promises
    // a +0.5 gain and bonds 1 USDC against that promise.
    let id = engine.open(TeachTerms {
        teacher_id: "sensei".into(),
        student_id: "kohai".into(),
        query_fee: Usdc::from_usdc(0.01),
        bond: Usdc::from_usdc(1.0),
        baseline_eval: 1.20,
        promised_gain: 0.50,
    })?;
    println!("session {id} open: baseline 1.20, promised gain +0.50, bond 1 USDC");

    // The student sends the states it is least sure about (high Q-variance);
    // each answer is one metered query of tuition.
    let uncertain_states: [&[f32]; 4] = [
        &[0.12, 0.88, 0.45],
        &[0.50, 0.49, 0.51],
        &[0.91, 0.10, 0.33],
        &[0.40, 0.41, 0.42],
    ];
    for state in uncertain_states {
        let answer = engine.ask(id, &teacher, state).await?;
        println!(
            "  queried {state:?} -> action {} (conviction {:.2})",
            answer.action, answer.conviction.0
        );
    }
    println!("tuition so far: {:.2} USDC", engine.tuition(id).as_usdc());

    // Case 1 — the student re-evaluates at 1.75: promise met, bond honored.
    let receipt = engine.close(id, 1.75)?;
    println!(
        "\nre-eval 1.75 -> {:?}: teacher keeps {:.2} USDC tuition and reclaims its bond",
        receipt.outcome,
        receipt.tuition.as_usdc()
    );

    // Case 2 — a second teacher overpromises and the student barely moves:
    // the bond slashes, refunding the student's tuition.
    let id = engine.open(TeachTerms {
        teacher_id: "charlatan".into(),
        student_id: "kohai".into(),
        query_fee: Usdc::from_usdc(0.01),
        bond: Usdc::from_usdc(1.0),
        baseline_eval: 1.75,
        promised_gain: 1.00,
    })?;
    for state in uncertain_states {
        engine.ask(id, &teacher, state).await?;
    }
    let receipt = engine.close(id, 1.80)?;
    assert_eq!(receipt.outcome, BondOutcome::Slashed);
    println!(
        "re-eval 1.80 vs promised 2.75 -> {:?}: student refunded {:.2} USDC from the bond",
        receipt.outcome,
        receipt.refund.as_usdc()
    );

    let ledger = engine.ledger();
    println!(
        "\nteaching ledger: {} honored / {} slashed (honor rate {:.0}%) — sellable reputation",
        ledger.honored,
        ledger.slashed,
        ledger.honor_rate() * 100.0
    );
    Ok(())
}
