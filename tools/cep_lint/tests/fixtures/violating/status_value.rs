// CEP:FILE: fixtures/violating/status_value.rs
// CEP:WHAT: Comment block with an invalid STATUS value.
// CEP:WHY: Exercises CEP-LINT-STATUS-VALUE.
// CEP:CLASS: CEP-2
// CEP:STATUS: done-ish
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

fn one(a: i32) -> i32 {
    return a;
}
