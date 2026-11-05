// CEP:FILE: crates/planck_ir/src/ir.rs
// CEP:WHAT: The Planck v0.1 intermediate representation: a typed control-flow graph of basic blocks with SSA temporaries and frame-slot locals.
// CEP:WHY: A linear CFG with explicit frame slots keeps phi insertion out of the correctness-critical path for this milestone while remaining fully deterministic, printable, and verifiable per standard 38.17; full Sea-of-Nodes floating and phi-based SSA are tracked as tickets PLK-002 and PLK-003.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: The verifier reports use-def, type, terminator, and linkage violations as structured findings; the IR itself never panics on well-formed input, and the verifier rejects malformed input instead of assuming validity.
// CEP:ASSUMES: Vregs are defined before use within a block or by a dominating block; slot indices are unique per function; all blocks are terminated; reference values are only produced by AddrSlot or loaded from reference-typed slots.
// CEP:COST: IR construction is O(program size); printing is O(nodes); verification is O(nodes * avg-def-scope) with dominator queries O(1) after O(N^2) worst-case bitset dominance build, O(N log N) typical (Cooper-Harvey-Kennedy).
// CEP:EVIDENCE: cargo test -p planck_ir; golden IR files in crates/planckc/tests.
// CEP:SECURITY: IR nodes carry no untrusted strings except symbol names, which are only ever printed or emitted into symbol tables, never executed.
// CEP:PORTABILITY: Pure core; target independence is preserved by keeping all machine-specific decisions in planck_codegen.

#![forbid(unsafe_code)]

/// Virtual register identifier. Unique per function, dense from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Vreg(pub u32);

/// Structure type identifier, dense from 0 across the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StructId(pub u32);

/// Enum type identifier, dense from 0 across the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EnumId(pub u32);

/// Frame slot identifier. Unique per function, dense from 0. Slots are byte-addressable locals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slot(pub u32);

/// Function identifier, dense from 0 across the whole program (user functions first, then no intrinsics; intrinsics are separate).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FuncId(pub u32);

/// Basic block identifier, dense from 0 per function. Block 0 is the entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

/// Runtime type of an IR value. Widths are exact; no target dependence here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Char,
    Unit,
    Str,
    /// Reference to a value of the inner type. `true` when declared `&mut`.
    Ref(Box<Ty>, bool),
    Struct(StructId),
    Enum(EnumId),
    Array(Box<Ty>, u64),
}

impl Ty {
    /// Size in bytes on the x86-64 System V target. Aggregates resolve through their layout tables.
    pub fn scalar_size(&self) -> Option<u32> {
        match self {
            Ty::I8 | Ty::U8 | Ty::Bool => Some(1),
            Ty::Char | Ty::I16 | Ty::U16 => Some(2),
            Ty::I32 | Ty::U32 | Ty::F32 => Some(4),
            Ty::I64 | Ty::U64 | Ty::F64 | Ty::Str | Ty::Ref(_, _) => Some(8),
            _ => None,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64
        )
    }

    pub fn is_signed(&self) -> bool {
        matches!(self, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Ty::F32 | Ty::F64)
    }

    pub fn is_scalar(&self) -> bool {
        self.scalar_size().is_some()
    }

    /// A stable name used by the printer and diagnostics.
    pub fn name(&self) -> String {
        match self {
            Ty::I8 => "i8".into(),
            Ty::I16 => "i16".into(),
            Ty::I32 => "i32".into(),
            Ty::I64 => "i64".into(),
            Ty::U8 => "u8".into(),
            Ty::U16 => "u16".into(),
            Ty::U32 => "u32".into(),
            Ty::U64 => "u64".into(),
            Ty::F32 => "f32".into(),
            Ty::F64 => "f64".into(),
            Ty::Bool => "bool".into(),
            Ty::Char => "char".into(),
            Ty::Unit => "()".into(),
            Ty::Str => "&str".into(),
            Ty::Ref(inner, true) => format!("&mut {}", inner.name()),
            Ty::Ref(inner, false) => format!("&{}", inner.name()),
            Ty::Struct(id) => format!("struct#{}", id.0),
            Ty::Enum(id) => format!("enum#{}", id.0),
            Ty::Array(elem, size) => format!("[{}; {}]", elem.name(), size),
        }
    }
}

/// Structure layout resolved during semantic analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct StructInfo {
    pub name: String,
    /// Fields in declaration order: (name, type, byte offset).
    pub fields: Vec<(String, Ty, u32)>,
    pub size: u32,
    pub align: u32,
}

/// Enum layout: 4-byte tag followed by payload storage.
#[derive(Clone, Debug, PartialEq)]
pub struct EnumInfo {
    pub name: String,
    /// Variants in declaration order: (name, optional payload type). Payload offset is uniform: align_up(4, payload_align).
    pub variants: Vec<(String, Option<Ty>)>,
    pub size: u32,
    pub align: u32,
    pub payload_offset: u32,
}

/// One read-only data blob (string literal bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct RodataEntry {
    pub label: String,
    pub bytes: Vec<u8>,
    /// Deterministic id for stable emission order.
    pub id: u32,
}

/// Runtime constants. Integers carry their IR type; strings index the rodata table.
#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Int(i128, Ty),
    Float(f64, bool),
    Bool(bool),
    Char(char),
    Str(u32),
}

/// Binary operations. Arithmetic/bitwise ops are typed; comparisons always produce Bool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            BinOp::Rem => "rem",
            BinOp::BitAnd => "and",
            BinOp::BitOr => "or",
            BinOp::BitXor => "xor",
            BinOp::Shl => "shl",
            BinOp::Shr => "shr",
            BinOp::Eq => "eq",
            BinOp::Ne => "ne",
            BinOp::Lt => "lt",
            BinOp::Le => "le",
            BinOp::Gt => "gt",
            BinOp::Ge => "ge",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }

    pub fn is_div_or_rem(self) -> bool {
        matches!(self, BinOp::Div | BinOp::Rem)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnOp {
    Neg,
    Not,
}

/// Builtin runtime entry points, resolved by semantic analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Intrinsic {
    Print,
    PrintStr,
    /// Halt with a message; used by trap lowering.
    Panic,
}

/// Aborts with a machine-readable trap reason. All traps exit(101) after printing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrapKind {
    Overflow,
    Bounds,
    DivZero,
    Panic,
}

/// One call argument: a value vreg plus its type, or a by-reference pointer for aggregate passing.
#[derive(Clone, Debug, PartialEq)]
pub struct CallArg {
    pub value: Vreg,
    pub ty: Ty,
    /// Aggregates larger than 8 bytes pass the address of a caller copy.
    pub by_ref: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    Const {
        dst: Vreg,
        value: Const,
    },
    Bin {
        op: BinOp,
        dst: Vreg,
        a: Vreg,
        b: Vreg,
        ty: Ty,
        checked: bool,
    },
    Un {
        op: UnOp,
        dst: Vreg,
        a: Vreg,
        ty: Ty,
        checked: bool,
    },
    Cast {
        dst: Vreg,
        src: Vreg,
        from: Ty,
        to: Ty,
    },
    LoadSlot {
        dst: Vreg,
        slot: Slot,
        ty: Ty,
    },
    StoreSlot {
        slot: Slot,
        src: Vreg,
        ty: Ty,
    },
    LoadField {
        dst: Vreg,
        slot: Slot,
        offset: u32,
        ty: Ty,
    },
    StoreField {
        slot: Slot,
        offset: u32,
        src: Vreg,
        ty: Ty,
    },
    /// Computes the address of a frame slot (a `&x` borrow).
    AddrSlot {
        dst: Vreg,
        slot: Slot,
        pointee: Ty,
        mutable: bool,
    },
    /// Computes `base + offset` where offset is a vreg holding a BYTE offset. Used for dynamic array indexing.
    AddrOffset {
        dst: Vreg,
        base: Vreg,
        offset: Vreg,
        pointee: Ty,
    },
    /// Loads through a reference value at a static byte offset.
    LoadIndirect {
        dst: Vreg,
        addr: Vreg,
        offset: u32,
        ty: Ty,
    },
    StoreIndirect {
        addr: Vreg,
        offset: u32,
        src: Vreg,
        ty: Ty,
    },
    /// Memcpy from an aggregate-valued address into a frame slot (aggregate assignment / binding).
    StoreStructFromAddr {
        dst_slot: Slot,
        src_addr: Vreg,
        ty: Ty,
    },
    Call {
        dst: Option<Vreg>,
        sret: Option<Vreg>,
        func: Callee,
        args: Vec<CallArg>,
        ret: Ty,
    },
    /// Conditional trap: aborts when `cond` is true.
    TrapIf {
        cond: Vreg,
        trap: TrapKind,
    },
    /// Unconditional trap.
    Trap {
        trap: TrapKind,
    },
    /// Marks a vreg as dead for DCE purposes only; pure bookkeeping introduced by lowering, never by user code.
    Comment {
        text: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Callee {
    Func(FuncId),
    Intrinsic(Intrinsic),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Br(BlockId),
    BrCond {
        cond: Vreg,
        then_blk: BlockId,
        else_blk: BlockId,
    },
    Ret {
        value: Option<Vreg>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlotInfo {
    pub ty: Ty,
    pub size: u32,
    pub align: u32,
    /// Human-readable origin: parameter name, local name, or compiler temporary tag.
    pub origin: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IrFunc {
    pub name: String,
    pub func_id: FuncId,
    /// Parameters in order: the slot holding the incoming value.
    pub params: Vec<Slot>,
    pub ret: Ty,
    /// For aggregate return types: the hidden slot holding the return buffer.
    pub sret_slot: Option<Slot>,
    pub slots: Vec<SlotInfo>,
    /// Type of every vreg, indexed by Vreg index.
    pub vregs: Vec<Ty>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub funcs: Vec<IrFunc>,
    pub structs: Vec<StructInfo>,
    pub enums: Vec<EnumInfo>,
    pub rodata: Vec<RodataEntry>,
    /// Semantics version recorded in dumps and future serialization.
    pub ir_version: String,
}

impl Program {
    pub fn find_func(&self, name: &str) -> Option<FuncId> {
        self.funcs
            .iter()
            .position(|f| f.name == name)
            .map(|idx| FuncId(idx as u32))
    }
}

impl IrFunc {
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn inst_count(&self) -> usize {
        self.blocks.iter().map(|b| b.insts.len()).sum()
    }
}
