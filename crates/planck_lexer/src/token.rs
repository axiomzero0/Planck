// CEP:FILE: crates/planck_lexer/src/token.rs
// CEP:WHAT: Token and literal value definitions for the Planck token stream.
// CEP:WHY: The parser matches on TokKind only; keeping literal payloads typed here prevents re-parsing text downstream.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: None; this module holds data definitions only.
// CEP:ASSUMES: IntLit values fit u64; FloatLit values are finite f64; the lexer rejects anything else.
// CEP:COST: All operations O(1); Clone allocates only for string payloads.
// CEP:EVIDENCE: exercised by every planck_lexer and planck_parser test.
// CEP:SECURITY: none.

use planck_support::Span;

/// Lexed literal payload with the parsed value already computed.
#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Int(u64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(String),
}

/// Token kind without payload; payload variants carry their parsed value.
#[derive(Clone, Debug, PartialEq)]
pub enum TokKind {
    Ident(String),
    Lit(Lit),
    // Keywords
    Fn,
    Let,
    Mut,
    If,
    Else,
    While,
    Loop,
    Break,
    Continue,
    For,
    In,
    Return,
    Struct,
    Enum,
    Impl,
    Match,
    As,
    True,
    False,
    SelfKw,
    // Symbols
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    PathSep,
    Arrow,
    FatArrow,
    Dot,
    DotDot,
    DotDotEq,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Amp,
    Pipe,
    Caret,
    Bang,
    Shl,
    Shr,
    Pound,
    // Attribute token: raw contents between `#[` and the matching `]`.
    Attr(String),
    Eof,
}

impl TokKind {
    /// Human-readable name used by parser diagnostics.
    pub fn describe(&self) -> String {
        match self {
            TokKind::Ident(name) => format!("identifier `{}`", name),
            TokKind::Lit(lit) => match lit {
                Lit::Int(_) => "integer literal".to_string(),
                Lit::Float(_) => "float literal".to_string(),
                Lit::Bool(_) => "boolean literal".to_string(),
                Lit::Char(_) => "character literal".to_string(),
                Lit::Str(_) => "string literal".to_string(),
            },
            TokKind::Fn => "`fn`".into(),
            TokKind::Let => "`let`".into(),
            TokKind::Mut => "`mut`".into(),
            TokKind::If => "`if`".into(),
            TokKind::Else => "`else`".into(),
            TokKind::While => "`while`".into(),
            TokKind::Loop => "`loop`".into(),
            TokKind::Break => "`break`".into(),
            TokKind::Continue => "`continue`".into(),
            TokKind::For => "`for`".into(),
            TokKind::In => "`in`".into(),
            TokKind::Return => "`return`".into(),
            TokKind::Struct => "`struct`".into(),
            TokKind::Enum => "`enum`".into(),
            TokKind::Impl => "`impl`".into(),
            TokKind::Match => "`match`".into(),
            TokKind::As => "`as`".into(),
            TokKind::True | TokKind::False => "boolean literal".into(),
            TokKind::SelfKw => "`self`".into(),
            TokKind::LParen => "`(`".into(),
            TokKind::RParen => "`)`".into(),
            TokKind::LBrace => "`{`".into(),
            TokKind::RBrace => "`}`".into(),
            TokKind::LBracket => "`[`".into(),
            TokKind::RBracket => "`]`".into(),
            TokKind::Comma => "`,`".into(),
            TokKind::Semi => "`;`".into(),
            TokKind::Colon => "`:`".into(),
            TokKind::PathSep => "`::`".into(),
            TokKind::Arrow => "`->`".into(),
            TokKind::FatArrow => "`=>`".into(),
            TokKind::Dot => "`.`".into(),
            TokKind::DotDot => "`..`".into(),
            TokKind::DotDotEq => "`..=`".into(),
            TokKind::Plus => "`+`".into(),
            TokKind::Minus => "`-`".into(),
            TokKind::Star => "`*`".into(),
            TokKind::Slash => "`/`".into(),
            TokKind::Percent => "`%`".into(),
            TokKind::Eq => "`=`".into(),
            TokKind::EqEq => "`==`".into(),
            TokKind::NotEq => "`!=`".into(),
            TokKind::Lt => "`<`".into(),
            TokKind::Le => "`<=`".into(),
            TokKind::Gt => "`>`".into(),
            TokKind::Ge => "`>=`".into(),
            TokKind::AndAnd => "`&&`".into(),
            TokKind::OrOr => "`||`".into(),
            TokKind::Amp => "`&`".into(),
            TokKind::Pipe => "`|`".into(),
            TokKind::Caret => "`^`".into(),
            TokKind::Bang => "`!`".into(),
            TokKind::Shl => "`<<`".into(),
            TokKind::Shr => "`>>`".into(),
            TokKind::Pound => "`#`".into(),
            TokKind::Attr(_) => "attribute".into(),
            TokKind::Eof => "end of file".into(),
        }
    }
}

/// One token: kind, parsed payload location, and source span.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokKind, span: Span) -> Self {
        Token { kind, span }
    }
}
