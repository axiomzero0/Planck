// CEP:FILE: fixtures/violating/empty_branch.rs
// CEP:WHAT: If statement with an empty block.
// CEP:WHY: Exercises CEP-LINT-EMPTY-BRANCH.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

fn guard(a: i32) -> () {
    if a == 0 { }
    return;
}
