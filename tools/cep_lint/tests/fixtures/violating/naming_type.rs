// CEP:FILE: fixtures/violating/naming_type.rs
// CEP:WHAT: Struct with a lowercase name.
// CEP:WHY: Exercises CEP-LINT-NAMING-TYPE.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

struct point {
    x: i32,
}

fn one(a: i32) -> i32 {
    return a;
}
