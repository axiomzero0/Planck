// CEP:FILE: fixtures/violating/constant_return.rs
// CEP:WHAT: Function whose body is a single constant return.
// CEP:WHY: Exercises CEP-LINT-CONSTANT-RETURN.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

fn always() -> bool {
    return true;
}
