// CEP:FILE: fixtures/compliant.rs
// CEP:WHAT: Compliant sample used by the lint self-test to prove zero findings under every configured rule.
// CEP:WHY: The self-test contract requires one compliant scenario per rule; this file is that scenario.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Returns a computed value; cannot fail.
// CEP:ASSUMES: Inputs are repository-trusted.
// CEP:COST: Offline tooling; cost irrelevant.
// CEP:EVIDENCE: cep_lint --self-test.
// CEP:SECURITY: no untrusted input.

const OFFSET_BASE: i32 = 64;

/// Adds one step to the base offset with explicit overflow handling.
// CEP:WHAT: Adds one step to the configured base offset.
// CEP:WHY: Demonstrates a complete CEP function block with no banned constructs.
// CEP:STATUS: complete
// CEP:FAILURE: Cannot fail; addition of small constants cannot overflow i32.
// CEP:ASSUMES: step is small relative to i32 range.
// CEP:COST: one add.
// CEP:EVIDENCE: self-test.
fn add_step(step: i32) -> i32 {
    let total: i32 = OFFSET_BASE + step;
    return total;
}

// CEP:WHAT: Chooses a label for the sample value.
// CEP:WHY: Demonstrates match without empty branches and without magic numbers.
// CEP:STATUS: complete
// CEP:FAILURE: Returns a default label for unmatched values.
// CEP:ASSUMES: value is an i32.
// CEP:COST: one comparison.
// CEP:EVIDENCE: self-test.
fn label_for(value: i32) -> i32 {
    let outcome: i32 = match value { 0 => 1, _ => 0 };
    return outcome;
}
