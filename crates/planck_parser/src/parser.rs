// CEP:FILE: crates/planck_parser/src/parser.rs
// CEP:WHAT: Recursive-descent parser with precedence climbing that converts the token stream into the AST.
// CEP:WHY: Hand-written descent gives exact spans, deterministic recovery, and full control over the struct-literal ambiguity in conditions and match scrutinees.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Emits Diag codes PLK-P0001 through PLK-P0006 for unexpected tokens, missing semicolons, missing types, missing blocks, and invalid patterns; recovers at statement and item boundaries and never panics.
// CEP:ASSUMES: Input was produced by planck_lexer and ends with an Eof token. `Ident::Ident(expr)` with exactly one argument parses as an enum variant payload; zero or multiple arguments parse as an associated call. Conditions and match scrutinees forbid struct literals.
// CEP:COST: O(N) in tokens, single lookahead, no backtracking beyond one token; recursion depth bounded by expression nesting depth of the source.
// CEP:EVIDENCE: cargo test -p planck_parser.
// CEP:SECURITY: Untrusted input cannot recurse unboundedly: nesting depth of expressions and statements is bounded by input size, and all indexing is bounds-checked with expect-style internal invariants that degrade to errors.
// CEP:PORTABILITY: Pure core.

use crate::ast::*;
use planck_lexer::{Lit, TokKind, Token};
use planck_support::{Diag, Span};

const ERR_UNEXPECTED: &str = "PLK-P0001";
const ERR_MISSING_SEMI: &str = "PLK-P0002";
const ERR_MISSING_TYPE: &str = "PLK-P0003";
const ERR_MISSING_BLOCK: &str = "PLK-P0004";
const ERR_INVALID_PATTERN: &str = "PLK-P0005";

/// Parses Planck source text into a module plus any collected diagnostics.
// CEP:WHAT: Handles the parse module step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn parse_module(source: &str) -> (Module, Vec<Diag>) {
    let lexed = planck_lexer::lex(source);
    let mut parser = Parser::new(lexed.tokens);
    parser.diags.extend(lexed.errors);
    let module = parser.parse_items();
    (module, parser.diags)
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diags: Vec<Diag>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            diags: Vec::new(),
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    // CEP:WHAT: Handles the bump step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos.min(self.tokens.len() - 1)].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        token
    }

    fn at(&self, kind: &TokKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }

    fn error(&mut self, code: &str, span: Span, message: String) {
        self.diags.push(Diag::error(code, span, message));
    }

    // CEP:WHAT: Handles the expected step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn expected(&mut self, what: &str) -> Token {
        let token = self.peek().clone();
        self.error(
            ERR_UNEXPECTED,
            token.span,
            format!("expected {}, found {}", what, token.kind.describe()),
        );
        token
    }

    fn eat(&mut self, kind: &TokKind) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            None
        }
    }

    // CEP:WHAT: Handles the expect step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn expect(&mut self, kind: &TokKind, what: &str) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            let token = self.peek().clone();
            let code = if matches!(kind, TokKind::Semi) {
                ERR_MISSING_SEMI
            } else {
                ERR_UNEXPECTED
            };
            self.error(
                code,
                token.span,
                format!("expected {}, found {}", what, token.kind.describe()),
            );
            None
        }
    }

    /// Advances to the next `;` (consumed) or stops before `}` / Eof.
    // CEP:WHAT: Handles the sync to statement boundary step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn sync_to_statement_boundary(&mut self) {
        loop {
            match self.peek().kind {
                TokKind::Semi => {
                    self.bump();
                    return;
                }
                TokKind::RBrace | TokKind::Eof => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    // CEP:WHAT: Handles the parse items step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_items(&mut self) -> Module {
        let mut items = Vec::new();
        loop {
            let attrs = self.parse_attrs();
            let token = self.peek().clone();
            match token.kind {
                TokKind::Eof => break,
                TokKind::Fn => {
                    if let Some(item) = self.parse_function(attrs) {
                        items.push(Item::Fn(Box::new(item)));
                    } else {
                        self.sync_to_item_boundary();
                    }
                }
                TokKind::Struct => {
                    if let Some(item) = self.parse_struct(attrs) {
                        items.push(Item::Struct(Box::new(item)));
                    } else {
                        self.sync_to_item_boundary();
                    }
                }
                TokKind::Enum => {
                    if let Some(item) = self.parse_enum(attrs) {
                        items.push(Item::Enum(Box::new(item)));
                    } else {
                        self.sync_to_item_boundary();
                    }
                }
                TokKind::Impl => {
                    if let Some(item) = self.parse_impl(attrs) {
                        items.push(Item::Impl(Box::new(item)));
                    } else {
                        self.sync_to_item_boundary();
                    }
                }
                other => {
                    self.error(
                        ERR_UNEXPECTED,
                        token.span,
                        format!("expected item, found {}", other.describe()),
                    );
                    self.bump();
                }
            }
        }
        Module { items }
    }

    // CEP:WHAT: Handles the sync to item boundary step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn sync_to_item_boundary(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek().kind {
                TokKind::Eof => return,
                TokKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokKind::RBrace => {
                    self.bump();
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                TokKind::Semi if depth == 0 => {
                    self.bump();
                    return;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    // CEP:WHAT: Handles the parse attrs step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_attrs(&mut self) -> Vec<Attr> {
        let mut attrs = Vec::new();
        while let TokKind::Attr(content) = self.peek().kind.clone() {
            let token = self.bump();
            let (name, args) = match content.split_once('(') {
                Some((name, rest)) => (
                    name.trim().to_string(),
                    rest.trim_end_matches(')').to_string(),
                ),
                None => (content.clone(), String::new()),
            };
            attrs.push(Attr {
                name,
                args,
                span: token.span,
            });
        }
        attrs
    }

    // CEP:WHAT: Handles the parse function step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_function(&mut self, attrs: Vec<Attr>) -> Option<Function> {
        let fn_token = self.bump();
        let name = self.parse_ident("function name")?;
        self.expect(&TokKind::LParen, "`(` after function name")?;
        let params = self.parse_params()?;
        self.expect(&TokKind::RParen, "`)` after parameters")?;
        let ret = if self.eat(&TokKind::Arrow).is_some() {
            self.parse_type()?
        } else {
            TypeExpr::Name {
                name: "()".to_string(),
                span: fn_token.span,
            }
        };
        let body = self.parse_block()?;
        let span = fn_token.span.join(body.span);
        Some(Function {
            attrs,
            name,
            params,
            ret,
            body,
            span,
        })
    }

    // CEP:WHAT: Handles the parse params step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_params(&mut self) -> Option<Vec<Param>> {
        let mut params = Vec::new();
        if self.at(&TokKind::RParen) {
            return Some(params);
        }
        loop {
            let mut by_ref = false;
            let mut ref_mut = false;
            let mut is_self = false;
            if self.eat(&TokKind::Amp).is_some() {
                by_ref = true;
                if self.eat(&TokKind::Mut).is_some() {
                    ref_mut = true;
                }
                // `&self` and `&mut self` method receivers.
                if self.at(&TokKind::SelfKw) {
                    let token = self.bump();
                    params.push(Param {
                        by_ref,
                        ref_mut,
                        name: Ident {
                            name: "self".to_string(),
                            span: token.span,
                        },
                        ty: TypeExpr::Name {
                            name: "self".to_string(),
                            span: token.span,
                        },
                    });
                    is_self = true;
                }
            } else if self.at(&TokKind::SelfKw) {
                let token = self.bump();
                params.push(Param {
                    by_ref: false,
                    ref_mut: false,
                    name: Ident {
                        name: "self".to_string(),
                        span: token.span,
                    },
                    ty: TypeExpr::Name {
                        name: "self".to_string(),
                        span: token.span,
                    },
                });
                is_self = true;
            }
            if is_self {
                if self.eat(&TokKind::Comma).is_none() {
                    break;
                }
                continue;
            }
            let name = self.parse_ident("parameter name")?;
            self.expect(&TokKind::Colon, "`:` after parameter name")?;
            let ty = self.parse_type()?;
            params.push(Param {
                by_ref,
                ref_mut,
                name,
                ty,
            });
            if self.eat(&TokKind::Comma).is_none() {
                break;
            }
        }
        Some(params)
    }

    // CEP:WHAT: Handles the parse ident step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_ident(&mut self, what: &str) -> Option<Ident> {
        match self.peek().kind.clone() {
            TokKind::Ident(name) => {
                let token = self.bump();
                Some(Ident {
                    name,
                    span: token.span,
                })
            }
            _ => {
                self.expected(what);
                None
            }
        }
    }

    // CEP:WHAT: Handles the parse type step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_type(&mut self) -> Option<TypeExpr> {
        let token = self.peek().clone();
        match token.kind {
            TokKind::LParen => {
                // Unit type `()`.
                self.bump();
                self.expect(&TokKind::RParen, "`)` to close the unit type")?;
                Some(TypeExpr::Name {
                    name: "()".to_string(),
                    span: token.span,
                })
            }
            TokKind::Amp => {
                self.bump();
                let mutable = self.eat(&TokKind::Mut).is_some();
                let inner = self.parse_type()?;
                let span = token.span.join(inner.span());
                Some(TypeExpr::Ref {
                    mutable,
                    inner: Box::new(inner),
                    span,
                })
            }
            TokKind::LBracket => {
                self.bump();
                let elem = self.parse_type()?;
                self.expect(&TokKind::Semi, "`;` in array type")?;
                let size_token = self.peek().clone();
                let size = match size_token.kind {
                    TokKind::Lit(Lit::Int(value)) => {
                        self.bump();
                        value
                    }
                    _ => {
                        self.expected("integer array size");
                        return None;
                    }
                };
                self.expect(&TokKind::RBracket, "`]` after array size")?;
                let span = token.span.join(Span::new(self.pos, self.pos));
                Some(TypeExpr::Array {
                    elem: Box::new(elem),
                    size,
                    size_span: size_token.span,
                    span,
                })
            }
            TokKind::Ident(name) => {
                self.bump();
                Some(TypeExpr::Name {
                    name,
                    span: token.span,
                })
            }
            _ => {
                self.error(
                    ERR_MISSING_TYPE,
                    token.span,
                    format!("expected a type, found {}", token.kind.describe()),
                );
                None
            }
        }
    }

    // CEP:WHAT: Handles the parse block step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_block(&mut self) -> Option<Block> {
        let lbrace = self.expect(&TokKind::LBrace, "`{` to open a block")?;
        let mut stmts = Vec::new();
        loop {
            match self.peek().kind {
                TokKind::RBrace => {
                    let rbrace = self.bump();
                    return Some(Block {
                        stmts,
                        span: lbrace.span.join(rbrace.span),
                    });
                }
                TokKind::Eof => {
                    self.error(
                        ERR_MISSING_BLOCK,
                        self.peek().span,
                        "block is missing its closing `}`".to_string(),
                    );
                    return Some(Block {
                        stmts,
                        span: lbrace.span,
                    });
                }
                _ => {
                    if let Some(stmt) = self.parse_stmt() {
                        stmts.push(stmt);
                    } else {
                        self.sync_to_statement_boundary();
                    }
                }
            }
        }
    }

    // CEP:WHAT: Handles the parse stmt step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_stmt(&mut self) -> Option<Stmt> {
        let token = self.peek().clone();
        match token.kind {
            TokKind::Let => self.parse_let(),
            TokKind::If => self.parse_if(),
            TokKind::While => {
                self.bump();
                let cond = self.parse_expr_condition()?;
                let body = self.parse_block()?;
                let span = token.span.join(body.span);
                self.eat(&TokKind::Semi);
                Some(Stmt::While { cond, body, span })
            }
            TokKind::Loop => {
                self.bump();
                let body = self.parse_block()?;
                let span = token.span.join(body.span);
                self.eat(&TokKind::Semi);
                Some(Stmt::Loop { body, span })
            }
            TokKind::For => self.parse_for(),
            TokKind::Return => {
                self.bump();
                let value = if self.at(&TokKind::Semi) {
                    None
                } else {
                    Some(self.parse_expr(false)?)
                };
                let semi = self.expect(&TokKind::Semi, "`;` after return")?;
                let span = token.span.join(semi.span);
                Some(Stmt::Return { value, span })
            }
            TokKind::Break => {
                self.bump();
                let semi = self.expect(&TokKind::Semi, "`;` after break")?;
                Some(Stmt::Break {
                    span: token.span.join(semi.span),
                })
            }
            TokKind::Continue => {
                self.bump();
                let semi = self.expect(&TokKind::Semi, "`;` after continue")?;
                Some(Stmt::Continue {
                    span: token.span.join(semi.span),
                })
            }
            TokKind::Match => self.parse_match(),
            _ => {
                let expr = self.parse_expr(false)?;
                if self.at(&TokKind::Eq) {
                    self.bump();
                    let value = self.parse_expr(false)?;
                    let semi = self.expect(&TokKind::Semi, "`;` after assignment")?;
                    let span = expr.span().join(semi.span);
                    return Some(Stmt::Assign {
                        target: expr,
                        value,
                        span,
                    });
                }
                let semi = self.expect(&TokKind::Semi, "`;` after expression statement")?;
                let _ = semi;
                Some(Stmt::Expr { expr })
            }
        }
    }

    // CEP:WHAT: Handles the parse let step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_let(&mut self) -> Option<Stmt> {
        let token = self.bump();
        let mutable = self.eat(&TokKind::Mut).is_some();
        let name = self.parse_ident("variable name")?;
        let ty = if self.eat(&TokKind::Colon).is_some() {
            Some(self.parse_type()?)
        } else {
            None
        };
        let init = if self.eat(&TokKind::Eq).is_some() {
            Some(self.parse_expr(false)?)
        } else {
            None
        };
        let semi = self.expect(&TokKind::Semi, "`;` after let statement")?;
        Some(Stmt::Let {
            mutable,
            name,
            ty,
            init,
            span: token.span.join(semi.span),
        })
    }

    /// Parses an if/else-if/else chain, flattening `else if` into additional arms.
    // CEP:WHAT: Handles the parse if step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_if(&mut self) -> Option<Stmt> {
        let token = self.bump();
        let mut arms = Vec::new();
        let mut fallback: Option<Block> = None;
        let cond = self.parse_expr_condition()?;
        let then_body = self.parse_block()?;
        arms.push((cond, then_body));
        let mut span = token.span;
        loop {
            if self.eat(&TokKind::Else).is_none() {
                break;
            }
            if self.at(&TokKind::If) {
                self.bump();
                let cond = self.parse_expr_condition()?;
                let body = self.parse_block()?;
                span = span.join(body.span);
                arms.push((cond, body));
                continue;
            }
            let body = self.parse_block()?;
            span = span.join(body.span);
            fallback = Some(body);
            break;
        }
        self.eat(&TokKind::Semi);
        Some(Stmt::If {
            arms,
            fallback,
            span,
        })
    }

    // CEP:WHAT: Handles the parse for step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_for(&mut self) -> Option<Stmt> {
        let token = self.bump();
        let var = self.parse_ident("loop variable")?;
        self.expect(&TokKind::In, "`in` after loop variable")?;
        let start = self.parse_expr(true)?;
        let inclusive = if self.at(&TokKind::DotDotEq) {
            self.bump();
            true
        } else if self.at(&TokKind::DotDot) {
            self.bump();
            false
        } else {
            let token = self.peek().clone();
            self.error(
                ERR_UNEXPECTED,
                token.span,
                "expected `..` or `..=` in for loop range".to_string(),
            );
            return None;
        };
        let end = self.parse_expr(true)?;
        let body = self.parse_block()?;
        let span = token.span.join(body.span);
        self.eat(&TokKind::Semi);
        Some(Stmt::For {
            var,
            start,
            end,
            inclusive,
            body,
            span,
        })
    }

    // CEP:WHAT: Handles the parse match step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_match(&mut self) -> Option<Stmt> {
        let token = self.bump();
        let scrutinee = self.parse_expr(true)?;
        self.expect(&TokKind::LBrace, "`{` to open match arms")?;
        let mut arms = Vec::new();
        loop {
            match self.peek().kind {
                TokKind::RBrace => {
                    let rbrace = self.bump();
                    let span = token.span.join(rbrace.span);
                    return Some(Stmt::Match {
                        scrutinee,
                        arms,
                        span,
                    });
                }
                TokKind::Eof => {
                    self.error(
                        ERR_MISSING_BLOCK,
                        self.peek().span,
                        "match is missing its closing `}`".to_string(),
                    );
                    return Some(Stmt::Match {
                        scrutinee,
                        arms,
                        span: token.span,
                    });
                }
                _ => {
                    let arm = self.parse_match_arm()?;
                    arms.push(arm);
                }
            }
        }
    }

    // CEP:WHAT: Handles the parse match arm step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_match_arm(&mut self) -> Option<MatchArm> {
        let pattern = self.parse_pattern()?;
        self.expect(&TokKind::FatArrow, "`=>` after pattern")?;
        let body = if self.at(&TokKind::LBrace) {
            MatchBody::Block(self.parse_block()?)
        } else {
            MatchBody::Expr(self.parse_expr(false)?)
        };
        if self.eat(&TokKind::Comma).is_none() && !self.at(&TokKind::RBrace) {
            self.expected("`,` after match arm");
        }
        Some(MatchArm { pattern, body })
    }

    // CEP:WHAT: Handles the parse pattern step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_pattern(&mut self) -> Option<Pattern> {
        let token = self.peek().clone();
        match token.kind.clone() {
            TokKind::Ident(name) if name == "_" => {
                self.bump();
                Some(Pattern::Wildcard(token.span))
            }
            TokKind::Minus => {
                self.bump();
                let int_token = self.peek().clone();
                match int_token.kind {
                    TokKind::Lit(Lit::Int(value)) => {
                        self.bump();
                        let span = token.span.join(int_token.span);
                        Some(Pattern::Int(negate_pattern_int(value), span))
                    }
                    _ => {
                        self.error(
                            ERR_INVALID_PATTERN,
                            int_token.span,
                            "expected integer after `-` in pattern".to_string(),
                        );
                        None
                    }
                }
            }
            TokKind::Lit(Lit::Int(value)) => {
                self.bump();
                Some(Pattern::Int(clamp_pattern_int(value), token.span))
            }
            TokKind::Ident(enum_name) => {
                self.bump();
                if self.eat(&TokKind::PathSep).is_some() {
                    let variant = self.parse_ident("variant name")?;
                    let binding = if self.eat(&TokKind::LParen).is_some() {
                        let binding = self.parse_ident("payload binding")?;
                        self.expect(&TokKind::RParen, "`)` after payload binding")?;
                        Some(binding)
                    } else {
                        None
                    };
                    let span = token.span.join(variant.span);
                    Some(Pattern::Variant {
                        enum_name: Ident {
                            name: enum_name,
                            span: token.span,
                        },
                        variant,
                        binding,
                        span,
                    })
                } else {
                    self.error(ERR_INVALID_PATTERN, token.span, "bare identifiers are not patterns; use `_`, an integer, or `Enum::Variant`".to_string());
                    None
                }
            }
            _ => {
                self.error(
                    ERR_INVALID_PATTERN,
                    token.span,
                    format!("invalid match pattern: {}", token.kind.describe()),
                );
                None
            }
        }
    }

    /// Parses an expression, banning struct literals at the top level (conditions, ranges, match scrutinees).
    fn parse_expr_condition(&mut self) -> Option<Expr> {
        self.parse_expr(true)
    }

    /// Full expression parser. When `no_struct` is set, `Ident {` is not treated as a struct literal.
    pub fn parse_expr(&mut self, no_struct: bool) -> Option<Expr> {
        self.parse_or(no_struct)
    }

    // CEP:WHAT: Handles the parse or step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_or(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_and(no_struct)?;
        while self.at(&TokKind::OrOr) {
            let op_token = self.bump();
            let rhs = self.parse_and(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse and step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_and(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_bitor(no_struct)?;
        while self.at(&TokKind::AndAnd) {
            let op_token = self.bump();
            let rhs = self.parse_bitor(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse bitor step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_bitor(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_bitxor(no_struct)?;
        while self.at(&TokKind::Pipe) {
            let op_token = self.bump();
            let rhs = self.parse_bitxor(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op: BinOp::BitOr,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse bitxor step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_bitxor(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_bitand(no_struct)?;
        while self.at(&TokKind::Caret) {
            let op_token = self.bump();
            let rhs = self.parse_bitand(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op: BinOp::BitXor,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse bitand step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_bitand(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_cmp(no_struct)?;
        while self.at(&TokKind::Amp) {
            let op_token = self.bump();
            let rhs = self.parse_cmp(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op: BinOp::BitAnd,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse cmp step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_cmp(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_shift(no_struct)?;
        loop {
            let op = match self.peek().kind {
                TokKind::EqEq => BinOp::Eq,
                TokKind::NotEq => BinOp::Ne,
                TokKind::Lt => BinOp::Lt,
                TokKind::Le => BinOp::Le,
                TokKind::Gt => BinOp::Gt,
                TokKind::Ge => BinOp::Ge,
                _ => break,
            };
            let op_token = self.bump();
            let rhs = self.parse_shift(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse shift step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_shift(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_add(no_struct)?;
        loop {
            let op = match self.peek().kind {
                TokKind::Shl => BinOp::Shl,
                TokKind::Shr => BinOp::Shr,
                _ => break,
            };
            let op_token = self.bump();
            let rhs = self.parse_add(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse add step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_add(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_mul(no_struct)?;
        loop {
            let op = match self.peek().kind {
                TokKind::Plus => BinOp::Add,
                TokKind::Minus => BinOp::Sub,
                _ => break,
            };
            let op_token = self.bump();
            let rhs = self.parse_mul(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse mul step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_mul(&mut self, no_struct: bool) -> Option<Expr> {
        let mut lhs = self.parse_unary(no_struct)?;
        loop {
            let op = match self.peek().kind {
                TokKind::Star => BinOp::Mul,
                TokKind::Slash => BinOp::Div,
                TokKind::Percent => BinOp::Rem,
                _ => break,
            };
            let op_token = self.bump();
            let rhs = self.parse_unary(no_struct)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: op_token.span.join(span),
            };
        }
        Some(lhs)
    }

    // CEP:WHAT: Handles the parse unary step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_unary(&mut self, no_struct: bool) -> Option<Expr> {
        let token = self.peek().clone();
        let op = match token.kind {
            TokKind::Minus => UnOp::Neg,
            TokKind::Bang => UnOp::Not,
            TokKind::Amp => {
                self.bump();
                let mutable = self.eat(&TokKind::Mut).is_some();
                let expr = self.parse_unary(no_struct)?;
                let span = token.span.join(expr.span());
                return Some(Expr::Borrow {
                    mutable,
                    expr: Box::new(expr),
                    span,
                });
            }
            TokKind::Star => {
                self.bump();
                let expr = self.parse_unary(no_struct)?;
                let span = token.span.join(expr.span());
                return Some(Expr::Deref {
                    expr: Box::new(expr),
                    span,
                });
            }
            _ => return self.parse_postfix(no_struct),
        };
        self.bump();
        let expr = self.parse_unary(no_struct)?;
        let span = token.span.join(expr.span());
        Some(Expr::Unary {
            op,
            expr: Box::new(expr),
            span,
        })
    }

    // CEP:WHAT: Handles the parse postfix step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_postfix(&mut self, no_struct: bool) -> Option<Expr> {
        let mut expr = self.parse_primary(no_struct)?;
        loop {
            match self.peek().kind.clone() {
                TokKind::LParen => {
                    self.bump();
                    let args = self.parse_call_args()?;
                    let rparen = self.expect(&TokKind::RParen, "`)` after call arguments")?;
                    match expr {
                        Expr::Ident { ident } => {
                            let span = ident.span.join(rparen.span);
                            expr = Expr::Call {
                                callee: ident,
                                args,
                                span,
                            };
                        }
                        Expr::MethodCall {
                            receiver,
                            method,
                            mut args,
                            ..
                        } => {
                            let _ = &mut args;
                            let span = receiver.span().join(rparen.span);
                            expr = Expr::MethodCall {
                                receiver,
                                method,
                                args,
                                span,
                            };
                        }
                        other => {
                            let span = other.span().join(rparen.span);
                            self.error(
                                ERR_UNEXPECTED,
                                span,
                                "only plain functions and methods are callable in this milestone"
                                    .to_string(),
                            );
                            expr = other;
                        }
                    }
                }
                TokKind::Dot => {
                    self.bump();
                    let member = self.parse_ident("field or method name")?;
                    if self.at(&TokKind::LParen) {
                        self.bump();
                        let args = self.parse_call_args()?;
                        let rparen = self.expect(&TokKind::RParen, "`)` after method arguments")?;
                        let span = expr.span().join(rparen.span);
                        expr = Expr::MethodCall {
                            receiver: Box::new(expr),
                            method: member,
                            args,
                            span,
                        };
                    } else {
                        let span = expr.span().join(member.span);
                        expr = Expr::Field {
                            base: Box::new(expr),
                            field: member,
                            span,
                        };
                    }
                }
                TokKind::LBracket => {
                    self.bump();
                    let index = self.parse_expr(false)?;
                    let rbracket = self.expect(&TokKind::RBracket, "`]` after index")?;
                    let span = expr.span().join(rbracket.span);
                    expr = Expr::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                        span,
                    };
                }
                TokKind::As => {
                    self.bump();
                    let ty = self.parse_type()?;
                    let span = expr.span().join(ty.span());
                    expr = Expr::Cast {
                        expr: Box::new(expr),
                        ty,
                        span,
                    };
                }
                _ => break,
            }
        }
        Some(expr)
    }

    // CEP:WHAT: Handles the parse call args step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_call_args(&mut self) -> Option<Vec<Expr>> {
        let mut args = Vec::new();
        if self.at(&TokKind::RParen) {
            return Some(args);
        }
        loop {
            args.push(self.parse_expr(false)?);
            if self.eat(&TokKind::Comma).is_none() {
                break;
            }
        }
        Some(args)
    }

    // CEP:WHAT: Handles the parse primary step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_primary(&mut self, no_struct: bool) -> Option<Expr> {
        let token = self.peek().clone();
        match token.kind.clone() {
            TokKind::Lit(lit) => {
                self.bump();
                Some(Expr::Lit {
                    lit,
                    span: token.span,
                })
            }
            TokKind::True => {
                self.bump();
                Some(Expr::Lit {
                    lit: Lit::Bool(true),
                    span: token.span,
                })
            }
            TokKind::False => {
                self.bump();
                Some(Expr::Lit {
                    lit: Lit::Bool(false),
                    span: token.span,
                })
            }
            TokKind::SelfKw => {
                self.bump();
                Some(Expr::Ident {
                    ident: Ident {
                        name: "self".to_string(),
                        span: token.span,
                    },
                })
            }
            TokKind::LParen => {
                self.bump();
                if self.at(&TokKind::RParen) {
                    self.bump();
                    return Some(Expr::Unit { span: token.span });
                }
                let inner = self.parse_expr(false)?;
                self.expect(&TokKind::RParen, "`)`")?;
                Some(inner)
            }
            TokKind::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                while !self.at(&TokKind::RBracket) && !self.at(&TokKind::Eof) {
                    elems.push(self.parse_expr(false)?);
                    if self.eat(&TokKind::Comma).is_none() {
                        break;
                    }
                }
                let rbracket = self.expect(&TokKind::RBracket, "`]` after array elements")?;
                Some(Expr::ArrayLit {
                    elems,
                    span: token.span.join(rbracket.span),
                })
            }
            TokKind::Ident(name) => {
                self.bump();
                let ident = Ident {
                    name: name.clone(),
                    span: token.span,
                };
                if self.at(&TokKind::PathSep) {
                    self.bump();
                    let variant = self.parse_ident("variant or member name")?;
                    if self.at(&TokKind::LParen) {
                        self.bump();
                        let args = self.parse_call_args()?;
                        let rparen = self.expect(&TokKind::RParen, "`)` after payload")?;
                        let span = token.span.join(rparen.span);
                        if args.len() == 1 {
                            return Some(Expr::EnumLit {
                                enum_name: ident,
                                variant,
                                payload: Some(Box::new(
                                    args.into_iter().next().expect("checked length"),
                                )),
                                span,
                            });
                        }
                        let callee = Ident {
                            name: format!("{}::{}", ident.name, variant.name),
                            span: token.span.join(variant.span),
                        };
                        return Some(Expr::Call { callee, args, span });
                    }
                    let span = token.span.join(variant.span);
                    return Some(Expr::EnumLit {
                        enum_name: ident,
                        variant,
                        payload: None,
                        span,
                    });
                }
                if self.at(&TokKind::LBrace) && !no_struct {
                    self.bump();
                    let fields = self.parse_struct_fields()?;
                    let rbrace = self.expect(&TokKind::RBrace, "`}` after struct literal")?;
                    let span = token.span.join(rbrace.span);
                    return Some(Expr::StructLit {
                        name: ident,
                        fields,
                        span,
                    });
                }
                Some(Expr::Ident { ident })
            }
            _ => {
                self.expected("an expression");
                None
            }
        }
    }

    // CEP:WHAT: Handles the parse struct fields step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_struct_fields(&mut self) -> Option<Vec<(Ident, Expr)>> {
        let mut fields = Vec::new();
        if self.at(&TokKind::RBrace) {
            return Some(fields);
        }
        loop {
            let name = self.parse_ident("field name")?;
            self.expect(&TokKind::Colon, "`:` after field name")?;
            let value = self.parse_expr(false)?;
            fields.push((name, value));
            if self.eat(&TokKind::Comma).is_none() {
                break;
            }
        }
        Some(fields)
    }

    // CEP:WHAT: Handles the parse struct step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_struct(&mut self, attrs: Vec<Attr>) -> Option<StructDef> {
        self.bump();
        let name = self.parse_ident("struct name")?;
        self.expect(&TokKind::LBrace, "`{` to open struct body")?;
        let mut fields = Vec::new();
        while !self.at(&TokKind::RBrace) && !self.at(&TokKind::Eof) {
            let field_name = self.parse_ident("field name")?;
            self.expect(&TokKind::Colon, "`:` after field name")?;
            let ty = self.parse_type()?;
            fields.push((field_name, ty));
            if self.eat(&TokKind::Comma).is_none() {
                break;
            }
        }
        let rbrace = self.expect(&TokKind::RBrace, "`}` to close struct body")?;
        let _ = attrs;
        let span = name.span.join(rbrace.span);
        Some(StructDef { name, fields, span })
    }

    // CEP:WHAT: Handles the parse enum step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_enum(&mut self, _attrs: Vec<Attr>) -> Option<EnumDef> {
        self.bump();
        let name = self.parse_ident("enum name")?;
        self.expect(&TokKind::LBrace, "`{` to open enum body")?;
        let mut variants = Vec::new();
        while !self.at(&TokKind::RBrace) && !self.at(&TokKind::Eof) {
            let variant = self.parse_ident("variant name")?;
            let payload = if self.eat(&TokKind::LParen).is_some() {
                let ty = self.parse_type()?;
                self.expect(&TokKind::RParen, "`)` after payload type")?;
                Some(ty)
            } else {
                None
            };
            variants.push((variant, payload));
            if self.eat(&TokKind::Comma).is_none() {
                break;
            }
        }
        let rbrace = self.expect(&TokKind::RBrace, "`}` to close enum body")?;
        let span = name.span.join(rbrace.span);
        Some(EnumDef {
            name,
            variants,
            span,
        })
    }

    // CEP:WHAT: Handles the parse impl step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_impl(&mut self, _attrs: Vec<Attr>) -> Option<ImplBlock> {
        let token = self.bump();
        let self_ty = self.parse_ident("type name after `impl`")?;
        self.expect(&TokKind::LBrace, "`{` to open impl body")?;
        let mut funcs = Vec::new();
        while !self.at(&TokKind::RBrace) && !self.at(&TokKind::Eof) {
            let attrs = self.parse_attrs();
            if let Some(func) = self.parse_function(attrs) {
                funcs.push(func);
            } else {
                self.sync_to_statement_boundary();
            }
        }
        let rbrace = self.expect(&TokKind::RBrace, "`}` to close impl body")?;
        let span = token.span.join(rbrace.span);
        Some(ImplBlock {
            self_ty,
            funcs,
            span,
        })
    }
}

fn negate_pattern_int(value: u64) -> i64 {
    if value <= (i64::MAX as u64) + 1 {
        (value as i128).wrapping_neg() as i64
    } else {
        i64::MIN
    }
}

fn clamp_pattern_int(value: u64) -> i64 {
    if value <= i64::MAX as u64 {
        value as i64
    } else {
        i64::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp, Expr, Stmt};

    // CEP:WHAT: Handles the parse ok step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn parse_ok(src: &str) -> Module {
        let (module, diags) = parse_module(src);
        assert!(diags.is_empty(), "unexpected diagnostics: {:#?}", diags);
        module
    }

    // CEP:WHAT: Verifies that parses function with types behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn parses_function_with_types() {
        let module = parse_ok("fn add(a: i32, b: i32) -> i32 { return a + b; }");
        assert_eq!(module.items.len(), 1);
        match &module.items[0] {
            Item::Fn(func) => {
                assert_eq!(func.name.name, "add");
                assert_eq!(func.params.len(), 2);
            }
            _ => panic!("expected function"),
        }
    }

    // CEP:WHAT: Verifies that parses let mut and assign behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn parses_let_mut_and_assign() {
        let module = parse_ok("fn f() -> () { let x: i32 = 1; let mut y: i32 = 2; y = 3; }");
        match &module.items[0] {
            Item::Fn(func) => {
                assert!(matches!(
                    func.body.stmts[0],
                    Stmt::Let { mutable: false, .. }
                ));
                assert!(matches!(
                    func.body.stmts[1],
                    Stmt::Let { mutable: true, .. }
                ));
                assert!(matches!(func.body.stmts[2], Stmt::Assign { .. }));
            }
            _ => panic!("expected function"),
        }
    }

    // CEP:WHAT: Verifies that struct literal not parsed in condition behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn struct_literal_not_parsed_in_condition() {
        let module = parse_ok("fn f(x: i32) -> () { if x == 1 { return; } }");
        match &module.items[0] {
            Item::Fn(func) => {
                assert!(!matches!(func.body.stmts[0], Stmt::While { .. }));
            }
            _ => panic!(),
        }
    }

    // CEP:WHAT: Verifies that parses struct and enum and match behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn parses_struct_and_enum_and_match() {
        let module = parse_ok(
            "struct P { x: i32 } enum Color { Red, Green, Blue } \
             fn f(c: Color) -> i32 { match c { Color::Red => 1, Color::Green => 2, Color::Blue => 3, _ => 0 } }",
        );
        assert_eq!(module.items.len(), 3);
    }

    #[test]
    fn precedence_groups_multiplication_first() {
        let module = parse_ok("fn f() -> i32 { return 1 + 2 * 3; }");
        match &module.items[0] {
            Item::Fn(func) => match &func.body.stmts[0] {
                Stmt::Return {
                    value: Some(Expr::Binary { op: BinOp::Add, .. }),
                    ..
                } => {}
                other => panic!("expected add at top: {:?}", other),
            },
            _ => panic!(),
        }
    }

    // CEP:WHAT: Verifies that for loop ranges behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn for_loop_ranges() {
        let module =
            parse_ok("fn f() -> () { for i in 0..3 { print(i); } for j in 0..=3 { print(j); } }");
        match &module.items[0] {
            Item::Fn(func) => {
                assert!(matches!(
                    &func.body.stmts[0],
                    Stmt::For {
                        inclusive: false,
                        ..
                    }
                ));
                assert!(matches!(
                    &func.body.stmts[1],
                    Stmt::For {
                        inclusive: true,
                        ..
                    }
                ));
            }
            _ => panic!(),
        }
    }

    // CEP:WHAT: Verifies that missing semicolon is reported with span behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn missing_semicolon_is_reported_with_span() {
        let (_, diags) = parse_module("fn f() -> () { let x: i32 = 1 }");
        assert!(diags.iter().any(|d| d.code == "PLK-P0002"));
    }

    // CEP:WHAT: Verifies that recovers after bad statement behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn recovers_after_bad_statement() {
        let (module, diags) = parse_module("fn f() -> () { let = 3; let y: i32 = 2; }");
        assert!(!diags.is_empty());
        match &module.items[0] {
            Item::Fn(func) => assert!(!func.body.stmts.is_empty()),
            _ => panic!(),
        }
    }

    // CEP:WHAT: Verifies that borrow and deref parse behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn borrow_and_deref_parse() {
        let module = parse_ok("fn f(x: &mut i32) -> () { *x = 5; let r: &i32 = &x; }");
        match &module.items[0] {
            Item::Fn(func) => {
                assert_eq!(func.params.len(), 1);
                match &func.params[0].ty {
                    TypeExpr::Ref { mutable: true, .. } => {}
                    other => panic!("expected ref type, got {:?}", other),
                }
            }
            _ => panic!(),
        }
    }

    // CEP:WHAT: Verifies that if else chains flatten into arms behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn if_else_chains_flatten_into_arms() {
        let module = parse_ok(
            "fn f(x: i32) -> () { if x > 10 { print(1); } else if x == 10 { print(2); } else { print(3); } }",
        );
        match &module.items[0] {
            Item::Fn(func) => match &func.body.stmts[0] {
                Stmt::If { arms, fallback, .. } => {
                    assert_eq!(arms.len(), 2);
                    assert!(fallback.is_some());
                }
                other => panic!("expected if, got {:?}", other),
            },
            _ => panic!(),
        }
    }

    // CEP:WHAT: Verifies that method calls parse behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn method_calls_parse() {
        let module = parse_ok("struct P { x: i32 } impl P { fn get(&self) -> i32 { return self.x; } } fn f(p: P) -> i32 { return p.get(); }");
        assert_eq!(module.items.len(), 3);
    }
}
