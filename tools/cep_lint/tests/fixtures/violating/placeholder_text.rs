// CEP:FILE: fixtures/violating/placeholder_text.rs
// CEP:WHAT: String literal containing placeholder text.
// CEP:WHY: Exercises CEP-LINT-PLACEHOLDER-TEXT.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: none
// CEP:ASSUMES: none
// CEP:COST: none
// CEP:EVIDENCE: self-test

fn one(a: i32) -> i32 {
    let message: &str = "not implemented yet";
    print(message);
    return a;
}

fn print(text: &str) -> () {
    return;
}
