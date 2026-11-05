// CEP:FILE: fixtures/violating/func_block.rs
// CEP:WHAT: Function without the required CEP comment block.
// CEP:WHY: Exercises CEP-LINT-FUNC-BLOCK.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test
fn two_statements(a: i32) -> i32 {
    let b: i32 = a + 1;
    return b;
}
