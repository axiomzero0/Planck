// CEP:FILE: crates/planck_parser/src/ast.rs
// CEP:WHAT: Abstract syntax tree for the v0.1 Planck core subset, with Display printers used by golden tests.
// CEP:WHY: Every later stage consumes this single tree shape; keeping it payload-typed and spanned avoids re-lexing and enables deterministic dumps.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: None; data definitions plus total printing functions only.
// CEP:ASSUMES: Spans refer to the token stream that produced the tree; integer patterns are i64-ranged; array sizes are integer literals.
// CEP:COST: Printing is O(tree size); Clone allocates only for owned strings.
// CEP:EVIDENCE: cargo test -p planck_parser (round-trip goldens).
// CEP:SECURITY: none.
// CEP:PORTABILITY: Pure core.

#![forbid(unsafe_code)]

use planck_lexer::Lit;
use planck_support::Span;
use std::fmt;

#[derive(Clone, Debug)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Fn(Box<Function>),
    Struct(Box<StructDef>),
    Enum(Box<EnumDef>),
    Impl(Box<ImplBlock>),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Fn(f) => f.span,
            Item::Struct(s) => s.span,
            Item::Enum(e) => e.span,
            Item::Impl(i) => i.span,
        }
    }
}

/// Parsed attribute: `#[name(args)]` where args may be empty.
#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    pub name: String,
    pub args: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub attrs: Vec<Attr>,
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    /// True when the parameter is `&mut name: T` or `mut name: T`.
    pub by_ref: bool,
    pub ref_mut: bool,
    pub name: Ident,
    pub ty: TypeExpr,
}

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl PartialEq for Ident {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let {
        mutable: bool,
        name: Ident,
        ty: Option<TypeExpr>,
        init: Option<Expr>,
        span: Span,
    },
    Expr {
        expr: Expr,
    },
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    If {
        arms: Vec<(Expr, Block)>,
        fallback: Option<Block>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    Loop {
        body: Block,
        span: Span,
    },
    For {
        var: Ident,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Block,
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Match {
        scrutinee: Expr,
        arms: Vec<MatchArm>,
        span: Span,
    },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Loop { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Break { span }
            | Stmt::Continue { span }
            | Stmt::Match { span, .. } => *span,
            Stmt::Expr { expr } => expr.span(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: MatchBody,
}

#[derive(Clone, Debug)]
pub enum MatchBody {
    Expr(Expr),
    Block(Block),
}

impl MatchBody {
    pub fn span(&self) -> Span {
        match self {
            MatchBody::Expr(expr) => expr.span(),
            MatchBody::Block(block) => block.span,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard(Span),
    Int(i64, Span),
    /// Enum variant reference, with an optional payload binding name.
    Variant {
        enum_name: Ident,
        variant: Ident,
        binding: Option<Ident>,
        span: Span,
    },
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(span) | Pattern::Int(_, span) => *span,
            Pattern::Variant { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Lit {
        lit: Lit,
        span: Span,
    },
    /// The unit value `()`.
    Unit {
        span: Span,
    },
    ArrayLit {
        elems: Vec<Expr>,
        span: Span,
    },
    Ident {
        ident: Ident,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Ident,
        args: Vec<Expr>,
        span: Span,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
        span: Span,
    },
    Field {
        base: Box<Expr>,
        field: Ident,
        span: Span,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Cast {
        expr: Box<Expr>,
        ty: TypeExpr,
        span: Span,
    },
    StructLit {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
        span: Span,
    },
    EnumLit {
        enum_name: Ident,
        variant: Ident,
        payload: Option<Box<Expr>>,
        span: Span,
    },
    Borrow {
        mutable: bool,
        expr: Box<Expr>,
        span: Span,
    },
    Deref {
        expr: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Lit { span, .. }
            | Expr::Unit { span }
            | Expr::ArrayLit { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Call { span, .. }
            | Expr::MethodCall { span, .. }
            | Expr::Field { span, .. }
            | Expr::Index { span, .. }
            | Expr::Cast { span, .. }
            | Expr::StructLit { span, .. }
            | Expr::EnumLit { span, .. }
            | Expr::Borrow { span, .. }
            | Expr::Deref { span, .. } => *span,
            Expr::Ident { ident } => ident.span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: Ident,
    pub fields: Vec<(Ident, TypeExpr)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumDef {
    pub name: Ident,
    /// Each variant holds at most one payload type, matching design section 2.6.
    pub variants: Vec<(Ident, Option<TypeExpr>)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImplBlock {
    pub self_ty: Ident,
    pub funcs: Vec<Function>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeExpr {
    Name {
        name: String,
        span: Span,
    },
    Ref {
        mutable: bool,
        inner: Box<TypeExpr>,
        span: Span,
    },
    Array {
        elem: Box<TypeExpr>,
        size: u64,
        size_span: Span,
        span: Span,
    },
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Name { span, .. }
            | TypeExpr::Ref { span, .. }
            | TypeExpr::Array { span, .. } => *span,
        }
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeExpr::Name { name, .. } => write!(f, "{}", name),
            TypeExpr::Ref { mutable, inner, .. } => {
                if *mutable {
                    write!(f, "&mut {}", inner)
                } else {
                    write!(f, "&{}", inner)
                }
            }
            TypeExpr::Array { elem, size, .. } => write!(f, "[{}; {}]", elem, size),
        }
    }
}
