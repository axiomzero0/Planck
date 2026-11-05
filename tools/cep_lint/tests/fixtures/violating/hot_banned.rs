// CEP:FILE: fixtures/violating/hot_banned.rs
// CEP:WHAT: CEP-0 file using a banned allocation token.
// CEP:WHY: Exercises CEP-LINT-HOT-BANNED.
// CEP:CLASS: CEP-0
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: measured elsewhere
// CEP:EVIDENCE: self-test

fn make() -> i32 {
    let v: i32 = 1;
    return v;
}

fn use_unwrap(a: i32) -> i32 {
    let r: i32 = a;
    return r;
}
