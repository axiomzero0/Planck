// CEP:FILE: fixtures/violating/magic_number.rs
// CEP:WHAT: Numeric literal outside a sanctioned context.
// CEP:WHY: Exercises CEP-LINT-MAGIC-NUMBER.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

fn scale(a: i32) -> i32 {
    let b: i32 = a * 64;
    return b;
}
