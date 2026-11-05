// CEP:FILE: fixtures/violating/stub_decl.rs
// CEP:WHAT: Stub declaration without OWNER and TICKET.
// CEP:WHY: Exercises CEP-LINT-STUB-DECL.
// CEP:CLASS: CEP-2
// CEP:STATUS: stub
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

fn one(a: i32) -> i32 {
    return a;
}
