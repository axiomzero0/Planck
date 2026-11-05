# Parser

## Abstract

The parser converts the token stream into a spanned abstract syntax tree using recursive descent with precedence climbing. Its inputs are tokens from the lexer; its outputs are a `Module` plus syntax diagnostics with codes PLK-P0001 through PLK-P0005. Parsing never aborts: errors recover at statement and item boundaries.

## Formal Definitions

```rust
pub struct Module { pub items: Vec<Item> }
pub enum Item { Fn(Box<Function>), Struct(Box<StructDef>), Enum(Box<EnumDef>), Impl(Box<ImplBlock>) }
pub enum Stmt { Let{..}, Expr{..}, Assign{..}, If{arms, fallback, span}, While{..}, Loop{..}, For{..}, Return{..}, Break{..}, Continue{..}, Match{..} }
pub enum Expr { Lit{..}, Unit{..}, ArrayLit{..}, Ident{..}, Unary{..}, Binary{..}, Call{..}, MethodCall{..}, Field{..}, Index{..}, Cast{..}, StructLit{..}, EnumLit{..}, Borrow{..}, Deref{..} }
```

Precedence, loosest first: `||`, `&&`, `|`, `^`, `&`, equality, comparison, shift, additive, multiplicative; unary `- ! & &mut *`; postfix call/field/index/method/`as`.

## Algorithm

1. Parse items until Eof; attributes attach to the following item.
2. Statements: keyword-dispatched for `let if while loop for return break continue match`; otherwise an expression statement or assignment.
3. Expressions: precedence climbing with one-token lookahead. `Ident {` parses as a struct literal except in conditions, ranges, and match scrutinees (the `no_struct` flag resolves the classic brace ambiguity).
4. Errors record a span and recover: statements sync to the next `;`, items sync past balanced braces.

Time complexity O(N) in tokens with no backtracking.

## Invariants

1. Every AST node carries a span within the source.
2. If chains flatten: `arms.len() >= 1` with `fallback` at most one block.
3. Match arms are patterns `=>` body; patterns are `_`, integers (optionally negated), or `Enum::Variant(binding?)`.
4. Parse errors always produce diagnostics; the returned module is best-effort but structurally traversable.

## Integration Points

- Depends on: `planck_lexer`, `planck_support`.
- Depended on by: `planck_sema` (`analyze` consumes `Module`).

Signature: `pub fn parse_module(source: &str) -> (Module, Vec<Diag>)`.

## Known Limitations

- `if` and `match` are statements, not expressions (PLK-022 tracks expression position).
- No closures, no destructuring patterns, no where-clauses.
- Associated-function calls parse `Ident::Ident(args)` with zero or two-plus arguments as calls and exactly one argument as a variant payload; the rule is documented in the parser assumptions.
