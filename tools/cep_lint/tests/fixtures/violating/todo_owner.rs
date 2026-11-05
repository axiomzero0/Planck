// CEP:FILE: fixtures/violating/todo_owner.rs
// CEP:WHAT: Comment with an anonymous TODO marker.
// CEP:WHY: Exercises the anonymous work marker rule.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

// TODO: fix this someday
fn one(a: i32) -> i32 {
    return a;
}
