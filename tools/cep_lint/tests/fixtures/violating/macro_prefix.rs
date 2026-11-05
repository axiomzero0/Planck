// CEP:FILE: fixtures/violating/macro_prefix.rs
// CEP:WHAT: Macro definition without the CEP_ prefix.
// CEP:WHY: Exercises CEP-LINT-MACRO-PREFIX.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

macro_rules! helper {
    () => { 1 };
}

fn one(a: i32) -> i32 {
    return a;
}
