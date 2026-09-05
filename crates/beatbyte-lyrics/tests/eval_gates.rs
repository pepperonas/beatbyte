//! The plan's quality gates (§2) against a `lyrics-eval` report.
//!
//! Running the evaluation itself takes the corpus and the model and
//! half an hour; the test reads the REPORT a `beatbyte-cli lyrics-eval
//! --out` run wrote, named by `BEATBYTE_LYRICS_EVAL_REPORT`, and fails
//! below the gates. Without the variable it skips — loudly, so a green
//! run is never mistaken for a measured one.

use beatbyte_lyrics::eval::{GATE_AAE_S, GATE_PCO_01, GATE_PCO_03, REPORT_SCHEMA, Report};

#[test]
fn the_report_passes_the_plans_gates() {
    let Some(path) = std::env::var_os("BEATBYTE_LYRICS_EVAL_REPORT") else {
        eprintln!(
            "SKIPPED: set BEATBYTE_LYRICS_EVAL_REPORT to a report from `beatbyte-cli lyrics-eval \
             --out` to check the plan's gates"
        );
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("cannot read `{}`: {e}", path.to_string_lossy());
    });
    let report: Report = serde_json::from_str(&text).expect("a lyrics-eval report");
    assert_eq!(report.schema, REPORT_SCHEMA);
    assert!(report.all.songs > 0, "an empty report proves nothing");
    for (language, agg) in &report.by_language {
        eprintln!(
            "{language}: {} song(s), AAE {:.3} s, PCO@0.1 {:.3}, PCO@0.3 {:.3}, coverage {:.3}",
            agg.songs, agg.aae_s, agg.pco_01, agg.pco_03, agg.coverage
        );
    }
    let all = &report.all;
    eprintln!(
        "ALL: {} song(s), AAE {:.3} s, PCO@0.1 {:.3}, PCO@0.3 {:.3}",
        all.songs, all.aae_s, all.pco_01, all.pco_03
    );
    assert!(
        all.aae_s < GATE_AAE_S,
        "AAE {:.3} s is not under {GATE_AAE_S}",
        all.aae_s
    );
    assert!(
        all.pco_03 > GATE_PCO_03,
        "PCO@0.3 {:.3} is not over {GATE_PCO_03}",
        all.pco_03
    );
    assert!(
        all.pco_01 > GATE_PCO_01,
        "PCO@0.1 {:.3} is not over {GATE_PCO_01}",
        all.pco_01
    );
    assert!(all.passes_gates());
}
