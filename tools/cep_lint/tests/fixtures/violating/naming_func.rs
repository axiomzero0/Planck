// CEP:FILE: fixtures/violating/naming_func.rs
// CEP:WHAT: Function with a PascalCase name.
// CEP:WHY: Exercises CEP-LINT-NAMING-FUNC.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

fn AddOne(a: i32) -> i32 {
    let b: i32 = a + 1;
    return b;
}
