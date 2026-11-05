// CEP:FILE: crates/planck_parser/src/lib.rs
// CEP:WHAT: Planck AST and recursive-descent parser producing a spanned, diagnostics-annotated module tree.
// CEP:WHY: The parser is the single authority on concrete syntax; later stages only see typed AST nodes and never re-tokenize.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Parse errors surface as Diag codes PLK-P0001 through PLK-P0005 with spans; parsing always returns the best-effort module so downstream stages can run once errors exist but the driver halts before codegen.
// CEP:ASSUMES: Grammar is the v0.1 Planck core subset: no traits, generics, comptime, try, unsafe, or extern blocks (tickets PLK-004 through PLK-009). Match is a statement, not an expression. `if` is a statement, not an expression.
// CEP:COST: O(N) in source bytes end to end; no backtracking; one-token lookahead.
// CEP:EVIDENCE: cargo test -p planck_parser (structure and recovery tests).
// CEP:SECURITY: Untrusted source cannot cause unbounded recursion in this crate; nesting is bounded by input length and every token access is bounds-checked.
// CEP:PORTABILITY: Pure core.

#![forbid(unsafe_code)]

pub mod ast;
mod parser;

pub use ast::{
    Attr, BinOp, Block, EnumDef, Expr, Function, Ident, ImplBlock, Item, MatchArm, MatchBody,
    Module, Param, Pattern, Stmt, StructDef, TypeExpr, UnOp,
};
pub use parser::parse_module;
