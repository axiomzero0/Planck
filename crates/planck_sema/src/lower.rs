// CEP:FILE: crates/planck_sema/src/lower.rs
// CEP:WHAT: Single-pass type checking, mutability checking, borrow checking, and AST-to-IR lowering.
// CEP:WHY: Checking and lowering share every type decision, so a single pass emits verified IR directly and cannot diverge from the checks that justify it.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: All user errors are reported as diagnostics with codes PLK-S00xx and lowering continues best-effort; the output is code-generated only when no errors exist. The pass itself never panics on any input tree.
// CEP:ASSUMES: Aggregate-typed expressions evaluate to a vreg holding the aggregate's ADDRESS (address-value convention). Borrows are statement-scoped unless bound to a `let`, in which case they live to the end of the enclosing block (ticket PLK-015 tracks full NLL regions). All types are Copy in this milestone (no heap types; ticket PLK-010), so move and copy semantics coincide. `let` requires an initializer (PLK-S0020). Match requires a wildcard arm (design 2.5). Integer literals take the expected integer type or default to i32; out-of-range literals are errors.
// CEP:COST: O(program size); each node visited once; scope and borrow sets are small per function. Deeply nested expressions recurse proportionally to nesting depth, which is bounded by input size.
// CEP:EVIDENCE: cargo test -p planck_sema; end-to-end programs in crates/planckc/tests.
// CEP:SECURITY: Source is untrusted: name lookups are total, recursion depth follows input nesting, and all index arithmetic uses saturating operations before the emitted runtime bounds checks take over.
// CEP:PORTABILITY: IR produced here is target-independent; layout constants follow x86-64 System V and are stated in types.rs.

#![forbid(unsafe_code)]

use crate::sigs::FnLocation;
use crate::types::TypeTables;
use planck_ir::{
    BinOp, Block, BlockId, Const, EnumId, FuncId, Inst, IrFunc, Program, RodataEntry, Slot,
    SlotInfo, StructId, Term, TrapKind, Ty, Vreg,
};
use planck_parser::{Block as AstBlock, Expr, Item, Module, Pattern, Stmt};
use planck_support::{Diag, Span};
use std::collections::BTreeMap;

const E_NOT_MUTABLE: &str = "PLK-S0005";
const E_BORROW: &str = "PLK-S0006";
pub const E_MATCH_NOT_EXHAUSTIVE: &str = "PLK-S0010";
pub const E_UNKNOWN_FIELD: &str = "PLK-S0011";
pub const E_UNKNOWN_VARIANT: &str = "PLK-S0012";
const E_NOT_BOOL: &str = "PLK-S0014";
const E_BAD_INDEX: &str = "PLK-S0015";
const E_UNEXPECTED_TYPE: &str = "PLK-S0003";
const E_BAD_OPERANDS: &str = "PLK-S0016";
const E_NO_INIT: &str = "PLK-S0020";
const E_UNKNOWN_NAME: &str = "PLK-S0002";

/// Result of semantic analysis.
pub struct SemaOutput {
    pub program: Program,
    pub diags: Vec<Diag>,
}

// CEP:WHAT: Handles the check and lower step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn check_and_lower(module: &Module, checked_overflow: bool) -> SemaOutput {
    let mut diags: Vec<Diag> = Vec::new();
    let tables = crate::types::build_tables(module, &mut diags);
    let fn_table = crate::sigs::collect_signatures(module, &tables, &mut diags);

    let mut program = Program {
        funcs: Vec::new(),
        structs: tables.structs.clone(),
        enums: tables.enums.clone(),
        rodata: Vec::new(),
        ir_version: planck_ir::IR_FORMAT_VERSION.to_string(),
    };

    let mut rodata: Vec<RodataEntry> = Vec::new();
    let mut str_ids: BTreeMap<String, u32> = BTreeMap::new();

    for reference in &fn_table.order {
        let pair: Option<(&planck_parser::Function, Option<String>)> = match &reference.location {
            FnLocation::TopLevel(idx) => match &module.items[*idx] {
                Item::Fn(func) => Some((func.as_ref(), None)),
                _ => None,
            },
            FnLocation::Method(impl_idx, method_idx) => match &module.items[*impl_idx] {
                Item::Impl(impl_block) => Some((
                    &impl_block.funcs[*method_idx],
                    Some(impl_block.self_ty.name.clone()),
                )),
                _ => None,
            },
        };
        let (func_ast, method_of) = match pair {
            Some(pair) => pair,
            None => continue,
        };
        let sig = match fn_table.sigs.get(&reference.qualified_name) {
            Some(sig) => sig.clone(),
            None => continue,
        };
        let mut builder = FnBuilder::new(
            &tables,
            &fn_table.sigs,
            &mut program,
            &mut rodata,
            &mut str_ids,
            &mut diags,
            checked_overflow,
            method_of,
        );
        let built = builder.build_function(func_ast, &sig);
        program.funcs.push(built);
    }

    program.rodata = rodata;
    SemaOutput { program, diags }
}

pub(crate) struct Binding {
    pub(crate) slot: Slot,
    pub(crate) ty: Ty,
    pub(crate) mutable: bool,
}

pub(crate) struct ActiveBorrow {
    root: Slot,
    pub(crate) mutable: bool,
    persistent: bool,
    name: String,
}

pub(crate) struct LoopCtx {
    break_target: BlockId,
    continue_target: BlockId,
}

/// A lowered value. For aggregate types, `vreg` holds the ADDRESS of the aggregate
/// and `ty` is the aggregate type (address-value convention; see module assumptions).
pub struct Lowered {
    pub vreg: Vreg,
    pub ty: Ty,
}

/// An lvalue resolved to a storage location.
pub enum LValue {
    /// A frame slot (possibly at a static byte offset for nested field paths).
    Slot {
        slot: Slot,
        offset: u32,
        ty: Ty,
        root: Slot,
        root_name: String,
    },
    /// An address held in a vreg (through a reference or dynamic array indexing), with a static byte offset.
    Indirect {
        addr: Vreg,
        offset: u32,
        ty: Ty,
        root: Slot,
        root_name: String,
    },
}

impl LValue {
    pub fn root(&self) -> Slot {
        match self {
            LValue::Slot { root, .. } | LValue::Indirect { root, .. } => *root,
        }
    }

    pub fn root_name(&self) -> &str {
        match self {
            LValue::Slot { root_name, .. } | LValue::Indirect { root_name, .. } => root_name,
        }
    }

    pub fn ty(&self) -> &Ty {
        match self {
            LValue::Slot { ty, .. } | LValue::Indirect { ty, .. } => ty,
        }
    }
}

pub struct FnBuilder<'a> {
    pub(crate) tables: &'a TypeTables,
    pub(crate) sigs: &'a BTreeMap<String, crate::sigs::FnSig>,
    #[allow(dead_code)] // reserved for late-stage cross-function tables
    pub(crate) program: &'a mut Program,
    pub(crate) rodata: &'a mut Vec<RodataEntry>,
    pub(crate) str_ids: &'a mut BTreeMap<String, u32>,
    pub(crate) diags: &'a mut Vec<Diag>,
    pub(crate) checked: bool,
    pub(crate) method_of: Option<String>,
    pub(crate) func: IrFunc,
    pub(crate) scopes: Vec<BTreeMap<String, Binding>>,
    pub(crate) borrows: Vec<ActiveBorrow>,
    pub(crate) borrow_marks: Vec<usize>,
    pub(crate) loops: Vec<LoopCtx>,
    pub(crate) cur: BlockId,
    pub(crate) dead: bool,
    pub(crate) next_vreg: u32,
    pub(crate) next_slot: u32,
    pub(crate) next_block: u32,
    /// Vreg -> string byte length for string literals (drives PrintStr lowering).
    pub(crate) str_lens: BTreeMap<u32, u32>,
}

impl<'a> FnBuilder<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        tables: &'a TypeTables,
        sigs: &'a BTreeMap<String, crate::sigs::FnSig>,
        program: &'a mut Program,
        rodata: &'a mut Vec<RodataEntry>,
        str_ids: &'a mut BTreeMap<String, u32>,
        diags: &'a mut Vec<Diag>,
        checked: bool,
        method_of: Option<String>,
    ) -> Self {
        FnBuilder {
            tables,
            sigs,
            program,
            rodata,
            str_ids,
            diags,
            checked,
            method_of,
            func: IrFunc {
                name: String::new(),
                func_id: FuncId(0),
                params: Vec::new(),
                ret: Ty::Unit,
                sret_slot: None,
                slots: Vec::new(),
                vregs: Vec::new(),
                blocks: vec![Block {
                    insts: Vec::new(),
                    term: Term::Br(BlockId(0)),
                }],
            },
            scopes: vec![BTreeMap::new()],
            borrows: Vec::new(),
            borrow_marks: Vec::new(),
            loops: Vec::new(),
            cur: BlockId(0),
            dead: false,
            next_vreg: 0,
            next_slot: 0,
            next_block: 1,
            str_lens: BTreeMap::new(),
        }
    }

    // ----- allocators -----

    // CEP:WHAT: Handles the vreg step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn vreg(&mut self, ty: Ty) -> Vreg {
        let id = self.next_vreg;
        self.next_vreg += 1;
        self.func.vregs.push(ty);
        Vreg(id)
    }

    // CEP:WHAT: Handles the slot step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn slot(&mut self, ty: Ty, origin: &str) -> Slot {
        let size = self.tables.type_size(&ty);
        let align = self.tables.type_align(&ty);
        let id = self.next_slot;
        self.next_slot += 1;
        self.func.slots.push(SlotInfo {
            ty,
            size,
            align,
            origin: origin.to_string(),
        });
        Slot(id)
    }

    // CEP:WHAT: Handles the block step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn block(&mut self) -> BlockId {
        let id = self.next_block;
        self.next_block += 1;
        self.func.blocks.push(Block {
            insts: Vec::new(),
            term: Term::Br(BlockId(0)),
        });
        BlockId(id)
    }

    // CEP:WHAT: Handles the emit step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn emit(&mut self, inst: Inst) {
        if self.dead {
            return;
        }
        self.func.blocks[self.cur.0 as usize].insts.push(inst);
    }

    // CEP:WHAT: Handles the set term step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn set_term(&mut self, term: Term) {
        if self.dead {
            return;
        }
        self.func.blocks[self.cur.0 as usize].term = term;
    }

    // CEP:WHAT: Handles the terminate and start step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn terminate_and_start(&mut self, term: Term) -> BlockId {
        if self.dead {
            return self.cur;
        }
        self.set_term(term);
        let fresh = self.block();
        self.cur = fresh;
        fresh
    }

    // ----- lookups -----

    pub(crate) fn lookup(&self, name: &str) -> Option<Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.get(name) {
                return Some(Binding {
                    slot: binding.slot,
                    ty: binding.ty.clone(),
                    mutable: binding.mutable,
                });
            }
        }
        None
    }

    pub(crate) fn struct_field_index(&self, id: StructId, field: &str) -> Option<(u32, Ty, u32)> {
        let info = self.tables.struct_ty(id);
        info.fields
            .iter()
            .enumerate()
            .find(|(_, (name, _, _))| name == field)
            .map(|(idx, (_, ty, offset))| (idx as u32, ty.clone(), *offset))
    }

    pub(crate) fn enum_variant_index(
        &self,
        id: EnumId,
        variant: &str,
    ) -> Option<(u32, Option<Ty>)> {
        let info = self.tables.enum_ty(id);
        info.variants
            .iter()
            .enumerate()
            .find(|(_, (name, _))| name == variant)
            .map(|(idx, (_, payload))| (idx as u32, payload.clone()))
    }

    // CEP:WHAT: Handles the intern str step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn intern_str(&mut self, value: &str) -> u32 {
        if let Some(id) = self.str_ids.get(value) {
            return *id;
        }
        let id = self.rodata.len() as u32;
        self.rodata.push(RodataEntry {
            label: format!("str_{}", id),
            bytes: value.as_bytes().to_vec(),
            id,
        });
        self.str_ids.insert(value.to_string(), id);
        id
    }

    // ----- borrow discipline -----

    // CEP:WHAT: Handles the create borrow step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn create_borrow(
        &mut self,
        root: Slot,
        root_name: &str,
        mutable: bool,
        persistent: bool,
        span: Span,
    ) {
        if mutable {
            if let Some(existing) = self
                .borrows
                .iter()
                .find(|b| b.root == root)
                .map(|b| (b.mutable, b.name.clone()))
            {
                let (existing_mutable, existing_name) = existing;
                let reason = if existing_mutable {
                    format!(
                        "cannot borrow `{}` as mutable: it is already mutably borrowed by `{}`",
                        root_name, existing_name
                    )
                } else {
                    format!(
                        "cannot borrow `{}` as mutable: it is already borrowed by `{}`",
                        root_name, existing_name
                    )
                };
                self.diags.push(Diag::error(E_BORROW, span, reason));
                return;
            }
        } else if let Some(existing) = self
            .borrows
            .iter()
            .find(|b| b.root == root && b.mutable)
            .map(|b| b.name.clone())
        {
            self.diags.push(Diag::error(
                E_BORROW,
                span,
                format!(
                    "cannot borrow `{}` as immutable: it is mutably borrowed by `{}`",
                    root_name, existing
                ),
            ));
            return;
        }
        self.borrows.push(ActiveBorrow {
            root,
            mutable,
            persistent,
            name: root_name.to_string(),
        });
    }

    /// Checks an owner access against active borrows. `write` also covers moves.
    // CEP:WHAT: Handles the check owner use step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn check_owner_use(&mut self, root: Slot, root_name: &str, write: bool, span: Span) {
        for borrow in &self.borrows {
            if borrow.root != root {
                continue;
            }
            if borrow.mutable {
                self.diags.push(Diag::error(
                    E_BORROW,
                    span,
                    format!(
                        "cannot {} `{}` while it is mutably borrowed by `{}`",
                        if write { "assign to" } else { "use" },
                        root_name,
                        borrow.name
                    ),
                ));
                return;
            }
            if write {
                self.diags.push(Diag::error(
                    E_BORROW,
                    span,
                    format!(
                        "cannot assign to `{}` while it is borrowed by `{}`",
                        root_name, borrow.name
                    ),
                ));
                return;
            }
        }
    }

    // ----- type helpers -----

    pub(crate) fn type_name(&self, ty: &Ty) -> String {
        match ty {
            Ty::Struct(id) => self.tables.struct_ty(*id).name.clone(),
            Ty::Enum(id) => self.tables.enum_ty(*id).name.clone(),
            other => other.name(),
        }
    }

    pub(crate) fn type_size(&self, ty: &Ty) -> u32 {
        self.tables.type_size(ty)
    }

    pub(crate) fn report_mismatch(&mut self, span: Span, expected: &Ty, actual: &Ty) {
        self.diags.push(Diag::error(
            E_UNEXPECTED_TYPE,
            span,
            format!(
                "type mismatch: expected `{}`, found `{}`",
                self.type_name(expected),
                self.type_name(actual)
            ),
        ));
    }

    // CEP:WHAT: Handles the build function step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn build_function(
        &mut self,
        ast: &planck_parser::Function,
        sig: &crate::sigs::FnSig,
    ) -> IrFunc {
        self.func.name = sig.name.clone();
        self.func.func_id = FuncId(sig.func_id);
        self.func.ret = sig.ret.clone();

        // Aggregate return types get a hidden sret slot as slot 0.
        if !matches!(self.func.ret, Ty::Unit) && !is_scalar_shaped(&self.func.ret) {
            let ret_ty = self.func.ret.clone();
            let slot = self.slot(ret_ty.clone(), "<sret>");
            self.func.sret_slot = Some(slot);
            self.func.params.push(slot);
        }

        // Bind parameters. For methods, sig.params[0] is the receiver; explicit
        // parameters start at index 1. The `self` binding is created when the receiver
        // parameter is visited.
        let mut sig_idx = if self.method_of.is_some() {
            1usize
        } else {
            0usize
        };
        let mut self_bound = false;
        for param in &ast.params {
            if crate::sigs::is_self_receiver(param) {
                if let Some(receiver_ty) = sig.params.first().cloned() {
                    let slot = self.slot(receiver_ty.clone(), "self");
                    // The receiver is the first ABI parameter (after any sret).
                    self.func.params.push(slot);
                    self.scopes[0].insert(
                        "self".to_string(),
                        Binding {
                            slot,
                            ty: receiver_ty,
                            mutable: false,
                        },
                    );
                }
                self_bound = true;
                continue;
            }
            let ty = sig.params.get(sig_idx).cloned().unwrap_or(Ty::Unit);
            sig_idx += 1;
            let slot = self.slot(ty.clone(), &param.name.name);
            self.scopes[0].insert(
                param.name.name.clone(),
                Binding {
                    slot,
                    ty,
                    mutable: false,
                },
            );
            self.func.params.push(slot);
        }
        if self.method_of.is_some() && !self_bound {
            if let Some(receiver_ty) = sig.params.first().cloned() {
                let slot = self.slot(receiver_ty.clone(), "self");
                self.func.params.push(slot);
                self.scopes[0].insert(
                    "self".to_string(),
                    Binding {
                        slot,
                        ty: receiver_ty,
                        mutable: false,
                    },
                );
            }
        }

        self.lower_block(&ast.body);
        self.finish()
    }

    // CEP:WHAT: Handles the finish step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn finish(&mut self) -> IrFunc {
        // Guarantee the current block is terminated.
        if matches!(
            self.func.blocks[self.cur.0 as usize].term,
            Term::Br(BlockId(0))
        ) {
            // Only report a missing return when the exit block is actually reachable.
            let reachable = self.compute_reachable();
            let exit_reachable = reachable[self.cur.0 as usize];
            if exit_reachable && self.func.ret != Ty::Unit {
                self.diags.push(Diag::error(
                    "PLK-S0022",
                    Span::new(0, 0),
                    format!(
                        "function `{}` can reach its end without returning a value",
                        self.func.name
                    ),
                ));
                self.emit(Inst::Trap {
                    trap: TrapKind::Panic,
                });
                self.set_term(Term::Ret { value: None });
            } else {
                self.terminate_and_start(self.fallthrough_return());
            }
        }
        // Drop unreachable blocks: compute reachability from bb0.
        let reachable = self.compute_reachable();
        let mut mapping: Vec<Option<u32>> = vec![None; self.func.blocks.len()];
        let mut kept: Vec<Block> = Vec::with_capacity(reachable.len());
        for (idx, block) in self.func.blocks.drain(..).enumerate() {
            if reachable[idx] {
                mapping[idx] = Some(kept.len() as u32);
                kept.push(block);
            }
        }
        for block in kept.iter_mut() {
            match &mut block.term {
                Term::Br(target) => {
                    if let Some(mapped) = mapping[target.0 as usize] {
                        *target = BlockId(mapped);
                    }
                }
                Term::BrCond {
                    then_blk, else_blk, ..
                } => {
                    if let Some(mapped) = mapping[then_blk.0 as usize] {
                        *then_blk = BlockId(mapped);
                    }
                    if let Some(mapped) = mapping[else_blk.0 as usize] {
                        *else_blk = BlockId(mapped);
                    }
                }
                Term::Ret { .. } => {}
            }
        }
        self.func.blocks = kept;
        self.func.clone()
    }

    pub(crate) fn fallthrough_return(&self) -> Term {
        // Falling off the end of a function: unit functions return unit; integer functions
        // return 0 (documented deviation, ticket PLK-021); aggregate functions return the
        // sret buffer as-is (zero-initialized by the caller copy).
        match &self.func.ret {
            Ty::Unit => Term::Ret { value: None },
            _ => Term::Ret { value: None },
        }
    }

    // CEP:WHAT: Handles the compute reachable step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn compute_reachable(&self) -> Vec<bool> {
        let n = self.func.blocks.len();
        let mut reachable = vec![false; n];
        let mut stack = vec![0u32];
        while let Some(node) = stack.pop() {
            let node = node as usize;
            if reachable[node] {
                continue;
            }
            reachable[node] = true;
            match &self.func.blocks[node].term {
                Term::Br(target) => stack.push(target.0),
                Term::BrCond {
                    then_blk, else_blk, ..
                } => {
                    stack.push(then_blk.0);
                    stack.push(else_blk.0);
                }
                Term::Ret { .. } => {}
            }
        }
        reachable
    }

    // ----- blocks and scopes -----

    // CEP:WHAT: Handles the lower block step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_block(&mut self, block: &AstBlock) {
        self.scopes.push(BTreeMap::new());
        self.borrow_marks.push(self.borrows.len());
        for stmt in &block.stmts {
            self.lower_stmt(stmt);
        }
        self.end_scope();
    }

    // CEP:WHAT: Handles the end scope step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn end_scope(&mut self) {
        // Drop persistent borrows created in this scope.
        if let Some(mark) = self.borrow_marks.pop() {
            self.borrows.truncate(mark);
        }
        self.scopes.pop();
    }

    // ----- statements -----

    // CEP:WHAT: Handles the lower stmt step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_stmt(&mut self, stmt: &Stmt) {
        if self.dead {
            return;
        }
        match stmt {
            Stmt::Let {
                mutable,
                name,
                ty,
                init,
                span,
            } => self.lower_let(*mutable, name, ty.as_ref(), init.as_ref(), *span),
            Stmt::Expr { expr } => {
                self.lower_expr(expr, None);
                self.clear_temp_borrows();
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                self.lower_assign(target, value, *span);
                self.clear_temp_borrows();
            }
            Stmt::If { arms, fallback, .. } => self.lower_if(arms, fallback.as_ref()),
            Stmt::While { cond, body, .. } => self.lower_while(cond, body),
            Stmt::Loop { body, .. } => self.lower_loop(body),
            Stmt::For {
                var,
                start,
                end,
                inclusive,
                body,
                ..
            } => self.lower_for(var, start, end, *inclusive, body),
            Stmt::Return { value, span } => self.lower_return(value.as_ref(), *span),
            Stmt::Break { span } => self.lower_break(*span),
            Stmt::Continue { span } => self.lower_continue(*span),
            Stmt::Match {
                scrutinee,
                arms,
                span,
            } => {
                self.lower_match(scrutinee, arms, *span);
                self.clear_temp_borrows();
            }
        }
    }

    pub(crate) fn clear_temp_borrows(&mut self) {
        self.borrows.retain(|b| b.persistent);
    }

    // CEP:WHAT: Handles the lower let step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_let(
        &mut self,
        mutable: bool,
        name: &planck_parser::Ident,
        ty: Option<&planck_parser::TypeExpr>,
        init: Option<&Expr>,
        span: Span,
    ) {
        let declared = ty.and_then(|t| self.tables.resolve(t, self.diags));
        let init_expr = match init {
            Some(expr) => expr,
            None => {
                self.diags.push(Diag::error(
                    E_NO_INIT,
                    span,
                    "`let` requires an initializer in this milestone (ticket PLK-010 covers deferred initialization)".to_string(),
                ));
                // Bind a unit fallback value so later code can still resolve the name.
                let slot = self.slot(Ty::Unit, &name.name);
                self.scopes
                    .last_mut()
                    .expect("scope stack never empty")
                    .insert(
                        name.name.clone(),
                        Binding {
                            slot,
                            ty: Ty::Unit,
                            mutable,
                        },
                    );
                return;
            }
        };
        let lowered = self.lower_expr(init_expr, declared.as_ref());
        let ty = declared.unwrap_or(lowered.ty.clone());
        if lowered.ty != ty {
            self.report_mismatch(init_expr.span(), &ty, &lowered.ty);
        }
        let slot = self.slot(ty.clone(), &name.name);
        self.store_into_slot(slot, &ty, &lowered, init_expr.span());
        // A let-bound borrow is persistent until the enclosing scope ends.
        if let Expr::Borrow {
            mutable: borrow_mut,
            expr: borrowed,
            ..
        } = init_expr
        {
            if let Some(root_name) = lvalue_root_name(borrowed) {
                if let Some(binding) = self.lookup(&root_name) {
                    let root = binding.slot;
                    let root_display = root_name.clone();
                    self.create_borrow(root, &root_display, *borrow_mut, true, span);
                }
            }
        }
        self.scopes
            .last_mut()
            .expect("scope stack never empty")
            .insert(name.name.clone(), Binding { slot, ty, mutable });
        // Temporary borrows from the initializer end with the statement; persistent
        // borrows registered above are retained.
        self.clear_temp_borrows();
    }

    // CEP:WHAT: Handles the store into slot step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn store_into_slot(&mut self, slot: Slot, ty: &Ty, value: &Lowered, span: Span) {
        match ty {
            Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _) => {
                self.emit(Inst::StoreStructFromAddr {
                    dst_slot: slot,
                    src_addr: value.vreg,
                    ty: ty.clone(),
                });
            }
            _ => {
                if value.ty != *ty {
                    self.report_mismatch(span, ty, &value.ty);
                }
                self.emit(Inst::StoreSlot {
                    slot,
                    src: value.vreg,
                    ty: ty.clone(),
                });
            }
        }
    }

    // CEP:WHAT: Handles the lower assign step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_assign(&mut self, target: &Expr, value: &Expr, span: Span) {
        let lvalue = match self.resolve_lvalue(target) {
            Some(lvalue) => lvalue,
            None => return,
        };
        // Mutability and borrow checks on the root. Writes through a reference
        // (LValue::Indirect) are governed by the reference's own mutability, checked
        // below; the owner-binding check applies only to direct slot writes.
        let root = lvalue.root();
        let root_name = lvalue.root_name().to_string();
        if matches!(lvalue, LValue::Slot { .. }) {
            let root_mutable = self
                .lookup(&root_name)
                .map(|binding| binding.mutable)
                .unwrap_or(false);
            if !root_mutable {
                self.diags.push(Diag::error(
                    E_NOT_MUTABLE,
                    span,
                    format!("cannot assign to `{}`: not declared as mutable", root_name),
                ));
                return;
            }
        }
        self.check_owner_use(root, &root_name, true, span);
        let target_ty = lvalue.ty().clone();
        // Through an immutable reference, writes are banned.
        if let LValue::Indirect { addr, .. } = &lvalue {
            let addr_ty = self.func.vregs[addr.0 as usize].clone();
            if let Ty::Ref(_, mutable) = addr_ty {
                if !mutable {
                    self.diags.push(Diag::error(
                        E_NOT_MUTABLE,
                        span,
                        "cannot write through an immutable reference".to_string(),
                    ));
                    return;
                }
            }
        }
        let lowered = self.lower_expr(value, Some(&target_ty));
        if lowered.ty != target_ty {
            self.report_mismatch(value.span(), &target_ty, &lowered.ty);
            return;
        }
        match lvalue {
            LValue::Slot {
                slot, offset, ty, ..
            } => {
                if offset == 0 {
                    self.store_into_slot(slot, &ty, &lowered, span);
                } else {
                    match &ty {
                        Ty::Struct(_) | Ty::Enum(_) => {
                            self.emit(Inst::StoreStructFromAddr {
                                dst_slot: slot,
                                src_addr: lowered.vreg,
                                ty: ty.clone(),
                            });
                        }
                        _ => {
                            self.emit(Inst::StoreField {
                                slot,
                                offset,
                                src: lowered.vreg,
                                ty,
                            });
                        }
                    }
                }
            }
            LValue::Indirect {
                addr, offset, ty, ..
            } => {
                match &ty {
                    Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _) => {
                        // Aggregate store through a pointer: memcpy at the static offset.
                        self.emit(Inst::StoreIndirect {
                            addr,
                            offset,
                            src: lowered.vreg,
                            ty: Ty::Ref(Box::new(ty.clone()), true),
                        });
                    }
                    _ => {
                        self.emit(Inst::StoreIndirect {
                            addr,
                            offset,
                            src: lowered.vreg,
                            ty,
                        });
                    }
                }
            }
        }
    }

    // CEP:WHAT: Handles the lower return step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_return(&mut self, value: Option<&Expr>, span: Span) {
        let ret_ty = self.func.ret.clone();
        match (&ret_ty, value) {
            (Ty::Unit, None) => {
                self.terminate_and_start(Term::Ret { value: None });
            }
            (Ty::Unit, Some(expr)) => {
                let lowered = self.lower_expr(expr, None);
                if lowered.ty != Ty::Unit {
                    self.report_mismatch(expr.span(), &Ty::Unit, &lowered.ty);
                }
                self.terminate_and_start(Term::Ret { value: None });
            }
            (expected, Some(expr)) => {
                if is_scalar_shaped(expected) {
                    let lowered = self.lower_expr(expr, Some(expected));
                    if lowered.ty != *expected {
                        self.report_mismatch(expr.span(), expected, &lowered.ty);
                        return;
                    }
                    self.terminate_and_start(Term::Ret {
                        value: Some(lowered.vreg),
                    });
                } else {
                    // Aggregate return: copy the value into the sret slot, then return unit.
                    let lowered = self.lower_expr(expr, Some(expected));
                    if lowered.ty != *expected {
                        self.report_mismatch(expr.span(), expected, &lowered.ty);
                        return;
                    }
                    if let Some(sret) = self.func.sret_slot {
                        self.emit(Inst::StoreStructFromAddr {
                            dst_slot: sret,
                            src_addr: lowered.vreg,
                            ty: expected.clone(),
                        });
                    }
                    self.terminate_and_start(Term::Ret { value: None });
                }
            }
            (expected, None) => {
                self.diags.push(Diag::error(
                    E_UNEXPECTED_TYPE,
                    span,
                    format!(
                        "`return` requires a value of type `{}`",
                        self.type_name(expected)
                    ),
                ));
                self.terminate_and_start(Term::Ret { value: None });
            }
        }
    }

    // CEP:WHAT: Handles the lower break step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_break(&mut self, span: Span) {
        match self.loops.last().map(|ctx| ctx.break_target) {
            Some(target) => {
                self.terminate_and_start(Term::Br(target));
            }
            None => {
                self.diags.push(Diag::error(
                    E_BAD_OPERANDS,
                    span,
                    "`break` outside of a loop".to_string(),
                ));
            }
        }
    }

    // CEP:WHAT: Handles the lower continue step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_continue(&mut self, span: Span) {
        match self.loops.last().map(|ctx| ctx.continue_target) {
            Some(target) => {
                self.terminate_and_start(Term::Br(target));
            }
            None => {
                self.diags.push(Diag::error(
                    E_BAD_OPERANDS,
                    span,
                    "`continue` outside of a loop".to_string(),
                ));
            }
        }
    }

    // CEP:WHAT: Handles the lower if step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_if(&mut self, arms: &[(Expr, AstBlock)], fallback: Option<&AstBlock>) {
        let mut end_blocks: Vec<BlockId> = Vec::new();
        for (index, (cond, body)) in arms.iter().enumerate() {
            let cond_value = self.lower_expr(cond, Some(&Ty::Bool));
            if cond_value.ty != Ty::Bool {
                self.diags.push(Diag::error(
                    E_NOT_BOOL,
                    cond.span(),
                    "if condition must be `bool`".to_string(),
                ));
            }
            let then_blk = self.block();
            let next_blk = self.block();
            self.terminate_and_start(Term::BrCond {
                cond: cond_value.vreg,
                then_blk,
                else_blk: next_blk,
            });
            // Then-arm. The arm's OWN block is registered for the join fixup; arms that
            // ended with a terminator (return/break) leave `cur` on a fresh unreachable
            // block whose extra join edge is pruned by reachability.
            self.cur = then_blk;
            self.lower_block(body);
            end_blocks.push(self.cur);
            // Chain the next condition (or the fallback) from next_blk.
            self.cur = next_blk;
            let is_last = index + 1 == arms.len();
            if is_last {
                if let Some(fallback_body) = fallback {
                    self.lower_block(fallback_body);
                    end_blocks.push(self.cur);
                } else {
                    end_blocks.push(self.cur);
                }
            }
        }
        // Join: every registered arm-end block branches here; blocks that already
        // terminated keep their real terminator.
        let join = self.block();
        for end in end_blocks {
            if matches!(self.func.blocks[end.0 as usize].term, Term::Br(BlockId(0))) {
                self.func.blocks[end.0 as usize].term = Term::Br(join);
            }
        }
        self.cur = join;
    }

    // CEP:WHAT: Handles the lower while step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_while(&mut self, cond: &Expr, body: &AstBlock) {
        let header = self.block();
        self.terminate_and_start(Term::Br(header));
        self.cur = header;
        let cond_value = self.lower_expr(cond, Some(&Ty::Bool));
        if cond_value.ty != Ty::Bool {
            self.diags.push(Diag::error(
                E_NOT_BOOL,
                cond.span(),
                "while condition must be `bool`".to_string(),
            ));
        }
        let body_blk = self.block();
        let exit_blk = self.block();
        self.terminate_and_start(Term::BrCond {
            cond: cond_value.vreg,
            then_blk: body_blk,
            else_blk: exit_blk,
        });
        self.clear_temp_borrows();
        self.cur = body_blk;
        self.loops.push(LoopCtx {
            break_target: exit_blk,
            continue_target: header,
        });
        self.lower_block(body);
        self.loops.pop();
        self.terminate_and_start(Term::Br(header));
        self.cur = exit_blk;
    }

    // CEP:WHAT: Handles the lower loop step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_loop(&mut self, body: &AstBlock) {
        let header = self.block();
        self.terminate_and_start(Term::Br(header));
        self.cur = header;
        let exit_blk = self.block();
        self.loops.push(LoopCtx {
            break_target: exit_blk,
            continue_target: header,
        });
        self.lower_block(body);
        self.loops.pop();
        self.terminate_and_start(Term::Br(header));
        self.cur = exit_blk;
    }

    // CEP:WHAT: Handles the lower for step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_for(
        &mut self,
        var: &planck_parser::Ident,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
        body: &AstBlock,
    ) {
        let start_value = self.lower_expr(start, None);
        if !start_value.ty.is_integer() {
            self.diags.push(Diag::error(
                E_BAD_OPERANDS,
                start.span(),
                "for loop range bounds must be integers".to_string(),
            ));
            return;
        }
        let end_value = self.lower_expr(end, Some(&start_value.ty));
        if end_value.ty != start_value.ty {
            self.report_mismatch(end.span(), &start_value.ty, &end_value.ty);
            return;
        }
        let iter_ty = start_value.ty.clone();
        let iter_slot = self.slot(iter_ty.clone(), &var.name);
        let end_slot = self.slot(iter_ty.clone(), "<range_end>");
        self.emit(Inst::StoreSlot {
            slot: iter_slot,
            src: start_value.vreg,
            ty: iter_ty.clone(),
        });
        self.emit(Inst::StoreSlot {
            slot: end_slot,
            src: end_value.vreg,
            ty: iter_ty.clone(),
        });
        self.clear_temp_borrows();

        let header = self.block();
        self.terminate_and_start(Term::Br(header));
        self.cur = header;
        // cond: i < end (or <=)
        let i_value = self.vreg(iter_ty.clone());
        self.emit(Inst::LoadSlot {
            dst: i_value,
            slot: iter_slot,
            ty: iter_ty.clone(),
        });
        let end_cmp = self.vreg(iter_ty.clone());
        self.emit(Inst::LoadSlot {
            dst: end_cmp,
            slot: end_slot,
            ty: iter_ty.clone(),
        });
        let cond = self.vreg(Ty::Bool);
        self.emit(Inst::Bin {
            op: if inclusive { BinOp::Le } else { BinOp::Lt },
            dst: cond,
            a: i_value,
            b: end_cmp,
            ty: iter_ty.clone(),
            checked: false,
        });
        let body_blk = self.block();
        let exit_blk = self.block();
        self.terminate_and_start(Term::BrCond {
            cond,
            then_blk: body_blk,
            else_blk: exit_blk,
        });

        self.cur = body_blk;
        self.scopes.push(BTreeMap::new());
        self.borrow_marks.push(self.borrows.len());
        self.scopes
            .last_mut()
            .expect("scope stack never empty")
            .insert(
                var.name.clone(),
                Binding {
                    slot: iter_slot,
                    ty: iter_ty.clone(),
                    mutable: false,
                },
            );
        let continue_target = self.block();
        self.loops.push(LoopCtx {
            break_target: exit_blk,
            continue_target,
        });
        self.lower_block(body);
        let _ = self.loops.pop();
        self.end_scope();
        self.terminate_and_start(Term::Br(continue_target));
        // The increment block: i += 1 (wrapping increment; the checked increment would trap on u64::MAX).
        self.cur = continue_target;
        let one = self.vreg(iter_ty.clone());
        self.emit(Inst::Const {
            dst: one,
            value: Const::Int(1, iter_ty.clone()),
        });
        let current = self.vreg(iter_ty.clone());
        self.emit(Inst::LoadSlot {
            dst: current,
            slot: iter_slot,
            ty: iter_ty.clone(),
        });
        let next = self.vreg(iter_ty.clone());
        self.emit(Inst::Bin {
            op: BinOp::Add,
            dst: next,
            a: current,
            b: one,
            ty: iter_ty.clone(),
            checked: false,
        });
        self.emit(Inst::StoreSlot {
            slot: iter_slot,
            src: next,
            ty: iter_ty.clone(),
        });
        self.terminate_and_start(Term::Br(header));
        self.cur = exit_blk;
    }

    // ----- match -----

    // CEP:WHAT: Handles the lower match step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[planck_parser::MatchArm],
        span: Span,
    ) {
        let scrutinee_value = self.lower_expr(scrutinee, None);
        let scrutinee_ty = scrutinee_value.ty.clone();
        match &scrutinee_ty {
            Ty::Enum(enum_id) => {
                self.lower_enum_match(scrutinee, scrutinee_value, *enum_id, arms, span)
            }
            ty if ty.is_integer() => self.lower_int_match(scrutinee_value, arms, span),
            other => {
                self.diags.push(Diag::error(
                    E_BAD_OPERANDS,
                    scrutinee.span(),
                    format!(
                        "match scrutinee must be an enum or integer, found `{}`",
                        self.type_name(other)
                    ),
                ));
            }
        }
    }

    // CEP:WHAT: Handles the match scrutinee slot step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn match_scrutinee_slot(&mut self, scrutinee: &Expr, value: &Lowered) -> Slot {
        // Reuse the variable's own slot when the scrutinee is a plain identifier.
        if let Expr::Ident { ident } = scrutinee {
            if let Some(binding) = self.lookup(&ident.name) {
                return binding.slot;
            }
        }
        let slot = self.slot(value.ty.clone(), "<match_tmp>");
        self.store_into_slot(slot, &value.ty, value, scrutinee.span());
        slot
    }

    // CEP:WHAT: Handles the lower enum match step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_enum_match(
        &mut self,
        scrutinee: &Expr,
        scrutinee_value: Lowered,
        enum_id: EnumId,
        arms: &[planck_parser::MatchArm],
        span: Span,
    ) {
        let slot = self.match_scrutinee_slot(scrutinee, &scrutinee_value);
        let enum_info = self.tables.enum_ty(enum_id);
        let payload_offset = enum_info.payload_offset;
        let tag = self.vreg(Ty::U32);
        self.emit(Inst::LoadField {
            dst: tag,
            slot,
            offset: 0,
            ty: Ty::U32,
        });

        let exit = self.block();
        let mut has_wildcard = false;
        for arm in arms {
            match &arm.pattern {
                Pattern::Wildcard(_) => {
                    has_wildcard = true;
                    self.lower_match_body(&arm.body, None, None, exit);
                }
                Pattern::Int(value, pat_span) => {
                    self.diags.push(Diag::error(
                        E_BAD_OPERANDS,
                        *pat_span,
                        format!(
                            "integer pattern `{}` does not match an enum scrutinee",
                            value
                        ),
                    ));
                }
                Pattern::Variant {
                    enum_name,
                    variant,
                    binding,
                    ..
                } => {
                    if enum_name.name != self.tables.enum_ty(enum_id).name {
                        self.diags.push(Diag::error(
                            E_UNKNOWN_VARIANT,
                            enum_name.span,
                            format!(
                                "`{}::` is not the matched enum `{}`",
                                enum_name.name,
                                self.tables.enum_ty(enum_id).name
                            ),
                        ));
                        continue;
                    }
                    let (variant_idx, payload_ty) =
                        match self.enum_variant_index(enum_id, &variant.name) {
                            Some(found) => found,
                            None => {
                                self.diags.push(Diag::error(
                                    E_UNKNOWN_VARIANT,
                                    variant.span,
                                    format!(
                                        "enum `{}` has no variant `{}`",
                                        self.tables.enum_ty(enum_id).name,
                                        variant.name
                                    ),
                                ));
                                continue;
                            }
                        };
                    if binding.is_some() != payload_ty.is_some() {
                        self.diags.push(Diag::error(
                            E_UNKNOWN_VARIANT,
                            variant.span,
                            "pattern payload binding must match the variant payload".to_string(),
                        ));
                        continue;
                    }
                    let tag_const = self.vreg(Ty::U32);
                    self.emit(Inst::Const {
                        dst: tag_const,
                        value: Const::Int(variant_idx as i128, Ty::U32),
                    });
                    let matches = self.vreg(Ty::Bool);
                    self.emit(Inst::Bin {
                        op: BinOp::Eq,
                        dst: matches,
                        a: tag,
                        b: tag_const,
                        ty: Ty::U32,
                        checked: false,
                    });
                    let arm_blk = self.block();
                    let next = self.block();
                    self.terminate_and_start(Term::BrCond {
                        cond: matches,
                        then_blk: arm_blk,
                        else_blk: next,
                    });
                    self.cur = arm_blk;
                    let payload = payload_ty
                        .as_ref()
                        .map(|ty| (ty.clone(), slot, payload_offset));
                    self.lower_match_body(&arm.body, binding.as_ref(), payload.as_ref(), exit);
                    self.cur = next;
                }
            }
        }
        if !has_wildcard {
            self.diags.push(Diag::error(
                E_MATCH_NOT_EXHAUSTIVE,
                span,
                "match is not exhaustive: a `_` arm is required in this milestone".to_string(),
            ));
        }
        // Fallthrough when no arm matched (only possible when wildcard is missing or not last).
        self.terminate_and_start(Term::Br(exit));
        self.cur = exit;
    }

    // CEP:WHAT: Handles the lower match body step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_match_body(
        &mut self,
        body: &planck_parser::MatchBody,
        binding: Option<&planck_parser::Ident>,
        payload: Option<&(Ty, Slot, u32)>,
        exit: BlockId,
    ) {
        self.scopes.push(BTreeMap::new());
        self.borrow_marks.push(self.borrows.len());
        if let (Some(binding_name), Some((payload_ty, slot, payload_offset))) = (binding, payload) {
            let (payload_ty, slot, payload_offset) = (payload_ty.clone(), *slot, *payload_offset);
            let payload_value = self.vreg(payload_ty.clone());
            self.emit(Inst::LoadField {
                dst: payload_value,
                slot,
                offset: payload_offset,
                ty: payload_ty.clone(),
            });
            let bind_slot = self.slot(payload_ty.clone(), &binding_name.name);
            self.emit(Inst::StoreSlot {
                slot: bind_slot,
                src: payload_value,
                ty: payload_ty.clone(),
            });
            self.scopes
                .last_mut()
                .expect("scope stack never empty")
                .insert(
                    binding_name.name.clone(),
                    Binding {
                        slot: bind_slot,
                        ty: payload_ty.clone(),
                        mutable: false,
                    },
                );
        }
        match body {
            planck_parser::MatchBody::Expr(expr) => {
                // Match-as-terminator semantics: in a function returning a value, an
                // expression arm returns that value; in unit functions it is evaluated
                // for effect (documented v0.1 rule; full expression-position match is
                // ticket PLK-022).
                if self.func.ret == Ty::Unit {
                    self.lower_expr(expr, None);
                    self.clear_temp_borrows();
                    self.end_scope();
                    self.terminate_and_start(Term::Br(exit));
                } else {
                    let ret_ty = self.func.ret.clone();
                    let value = self.lower_expr(expr, Some(&ret_ty));
                    if value.ty != ret_ty {
                        self.report_mismatch(expr.span(), &ret_ty, &value.ty);
                    }
                    self.clear_temp_borrows();
                    self.end_scope();
                    self.terminate_and_start(Term::Ret {
                        value: Some(value.vreg),
                    });
                }
            }
            planck_parser::MatchBody::Block(block) => {
                self.lower_block(block);
                self.terminate_and_start(Term::Br(exit));
            }
        }
    }

    // CEP:WHAT: Handles the lower int match step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_int_match(
        &mut self,
        scrutinee_value: Lowered,
        arms: &[planck_parser::MatchArm],
        span: Span,
    ) {
        let ty = scrutinee_value.ty.clone();
        let slot = self.match_scrutinee_slot_from_value(&scrutinee_value, &ty);
        let exit = self.block();
        let mut has_wildcard = false;
        for arm in arms {
            match &arm.pattern {
                Pattern::Wildcard(_) => {
                    has_wildcard = true;
                    self.lower_match_body_simple(&arm.body, None, exit);
                }
                Pattern::Int(value, _pat_span) => {
                    let scrut = self.vreg(ty.clone());
                    self.emit(Inst::LoadSlot {
                        dst: scrut,
                        slot,
                        ty: ty.clone(),
                    });
                    let pat = self.vreg(ty.clone());
                    self.emit(Inst::Const {
                        dst: pat,
                        value: Const::Int(*value as i128, ty.clone()),
                    });
                    let matches = self.vreg(Ty::Bool);
                    self.emit(Inst::Bin {
                        op: BinOp::Eq,
                        dst: matches,
                        a: scrut,
                        b: pat,
                        ty: ty.clone(),
                        checked: false,
                    });
                    let arm_blk = self.block();
                    let next = self.block();
                    self.terminate_and_start(Term::BrCond {
                        cond: matches,
                        then_blk: arm_blk,
                        else_blk: next,
                    });
                    self.cur = arm_blk;
                    self.lower_match_body_simple(&arm.body, None, exit);
                    self.cur = next;
                }
                Pattern::Variant { enum_name, .. } => {
                    self.diags.push(Diag::error(
                        E_BAD_OPERANDS,
                        enum_name.span,
                        "enum pattern cannot match an integer scrutinee".to_string(),
                    ));
                }
            }
        }
        if !has_wildcard {
            self.diags.push(Diag::error(
                E_MATCH_NOT_EXHAUSTIVE,
                span,
                "match is not exhaustive: a `_` arm is required in this milestone".to_string(),
            ));
        }
        self.terminate_and_start(Term::Br(exit));
        self.cur = exit;
    }

    // CEP:WHAT: Handles the match scrutinee slot from value step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn match_scrutinee_slot_from_value(&mut self, value: &Lowered, ty: &Ty) -> Slot {
        let slot = self.slot(ty.clone(), "<match_tmp>");
        self.emit(Inst::StoreSlot {
            slot,
            src: value.vreg,
            ty: ty.clone(),
        });
        slot
    }

    // CEP:WHAT: Handles the lower match body simple step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn lower_match_body_simple(
        &mut self,
        body: &planck_parser::MatchBody,
        _binding: Option<&planck_parser::Ident>,
        exit: BlockId,
    ) {
        self.scopes.push(BTreeMap::new());
        self.borrow_marks.push(self.borrows.len());
        match body {
            planck_parser::MatchBody::Expr(expr) => {
                if self.func.ret == Ty::Unit {
                    self.lower_expr(expr, None);
                    self.clear_temp_borrows();
                    self.end_scope();
                    self.terminate_and_start(Term::Br(exit));
                } else {
                    let ret_ty = self.func.ret.clone();
                    let value = self.lower_expr(expr, Some(&ret_ty));
                    if value.ty != ret_ty {
                        self.report_mismatch(expr.span(), &ret_ty, &value.ty);
                    }
                    self.clear_temp_borrows();
                    self.end_scope();
                    self.terminate_and_start(Term::Ret {
                        value: Some(value.vreg),
                    });
                }
            }
            planck_parser::MatchBody::Block(block) => {
                self.lower_block(block);
                self.terminate_and_start(Term::Br(exit));
            }
        }
    }

    // ----- lvalues -----

    // CEP:WHAT: Handles the resolve lvalue step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn resolve_lvalue(&mut self, expr: &Expr) -> Option<LValue> {
        match expr {
            Expr::Ident { ident } => {
                let binding = match self.lookup(&ident.name) {
                    Some(binding) => binding,
                    None => {
                        self.diags.push(Diag::error(
                            E_UNKNOWN_NAME,
                            ident.span,
                            format!("unknown variable `{}`", ident.name),
                        ));
                        return None;
                    }
                };
                Some(LValue::Slot {
                    slot: binding.slot,
                    offset: 0,
                    ty: binding.ty,
                    root: binding.slot,
                    root_name: ident.name.clone(),
                })
            }
            Expr::Field { base, field, span } => {
                let mut base_lvalue = self.resolve_lvalue(base)?;
                // Auto-deref: field access through a reference reads the reference first,
                // preserving the reference's mutability for later write checks.
                if matches!(base_lvalue.ty(), Ty::Ref(_, _)) {
                    let root = base_lvalue.root();
                    let root_name = base_lvalue.root_name().to_string();
                    let (addr, pointee, ref_mutable) = match base_lvalue.ty().clone() {
                        Ty::Ref(pointee, ref_mutable) => {
                            let loaded = self.vreg(Ty::Ref(pointee.clone(), ref_mutable));
                            self.read_slot_value(&base_lvalue, loaded);
                            (loaded, *pointee, ref_mutable)
                        }
                        _ => unreachable!("checked above"),
                    };
                    let _ = ref_mutable;
                    base_lvalue = LValue::Indirect {
                        addr,
                        offset: 0,
                        ty: pointee,
                        root,
                        root_name,
                    };
                }
                let base_ty = base_lvalue.ty().clone();
                let struct_id = match &base_ty {
                    Ty::Struct(id) => *id,
                    other => {
                        self.diags.push(Diag::error(
                            E_UNKNOWN_FIELD,
                            *span,
                            format!(
                                "field access requires a struct, found `{}`",
                                self.type_name(other)
                            ),
                        ));
                        return None;
                    }
                };
                let (_idx, field_ty, offset) = match self.struct_field_index(struct_id, &field.name)
                {
                    Some(found) => found,
                    None => {
                        self.diags.push(Diag::error(
                            E_UNKNOWN_FIELD,
                            field.span,
                            format!(
                                "struct `{}` has no field `{}`",
                                self.tables.struct_ty(struct_id).name,
                                field.name
                            ),
                        ));
                        return None;
                    }
                };
                Some(match base_lvalue {
                    LValue::Slot {
                        slot,
                        offset: base_offset,
                        root,
                        root_name,
                        ..
                    } => LValue::Slot {
                        slot,
                        offset: base_offset.saturating_add(offset),
                        ty: field_ty,
                        root,
                        root_name,
                    },
                    LValue::Indirect {
                        addr,
                        offset: base_offset,
                        root,
                        root_name,
                        ..
                    } => LValue::Indirect {
                        addr,
                        offset: base_offset.saturating_add(offset),
                        ty: field_ty,
                        root,
                        root_name,
                    },
                })
            }
            Expr::Index { base, index, span } => {
                let base_lvalue = self.resolve_lvalue(base)?;
                let base_ty = base_lvalue.ty().clone();
                let (elem_ty, elem_size, elem_count) = match &base_ty {
                    Ty::Array(elem, count) => {
                        (elem.as_ref().clone(), self.tables.type_size(elem), *count)
                    }
                    other => {
                        self.diags.push(Diag::error(
                            E_BAD_INDEX,
                            *span,
                            format!(
                                "indexing requires an array, found `{}`",
                                self.type_name(other)
                            ),
                        ));
                        return None;
                    }
                };
                let index_value = self.lower_expr(index, None);
                if !index_value.ty.is_integer() {
                    self.diags.push(Diag::error(
                        E_BAD_INDEX,
                        index.span(),
                        "array index must be an integer".to_string(),
                    ));
                    return None;
                }
                // Bounds checks are always on in this milestone (design Part 3).
                let index_u64 = self.cast_for_bounds(&index_value);
                if index_value.ty.is_signed() {
                    let zero = self.vreg(index_value.ty.clone());
                    self.emit(Inst::Const {
                        dst: zero,
                        value: Const::Int(0, index_value.ty.clone()),
                    });
                    let negative = self.vreg(Ty::Bool);
                    self.emit(Inst::Bin {
                        op: BinOp::Lt,
                        dst: negative,
                        a: index_value.vreg,
                        b: zero,
                        ty: index_value.ty.clone(),
                        checked: false,
                    });
                    self.emit(Inst::TrapIf {
                        cond: negative,
                        trap: TrapKind::Bounds,
                    });
                }
                let count_vreg = self.vreg(Ty::U64);
                self.emit(Inst::Const {
                    dst: count_vreg,
                    value: Const::Int(elem_count as i128, Ty::U64),
                });
                let out_of_bounds = self.vreg(Ty::Bool);
                self.emit(Inst::Bin {
                    op: BinOp::Ge,
                    dst: out_of_bounds,
                    a: index_u64,
                    b: count_vreg,
                    ty: Ty::U64,
                    checked: false,
                });
                self.emit(Inst::TrapIf {
                    cond: out_of_bounds,
                    trap: TrapKind::Bounds,
                });
                // Address: base + index * elem_size.
                let elem_size_vreg = self.vreg(Ty::U64);
                self.emit(Inst::Const {
                    dst: elem_size_vreg,
                    value: Const::Int(elem_size as i128, Ty::U64),
                });
                let byte_offset = self.vreg(Ty::U64);
                self.emit(Inst::Bin {
                    op: BinOp::Mul,
                    dst: byte_offset,
                    a: index_u64,
                    b: elem_size_vreg,
                    ty: Ty::U64,
                    checked: false,
                });
                let root = base_lvalue.root();
                let root_name = base_lvalue.root_name().to_string();
                let addr = match &base_lvalue {
                    LValue::Slot { slot, .. } => {
                        let base_addr = self.vreg(Ty::Ref(Box::new(base_ty.clone()), false));
                        self.emit(Inst::AddrSlot {
                            dst: base_addr,
                            slot: *slot,
                            pointee: base_ty.clone(),
                            mutable: true,
                        });
                        let addr = self.vreg(Ty::Ref(Box::new(elem_ty.clone()), true));
                        self.emit(Inst::AddrOffset {
                            dst: addr,
                            base: base_addr,
                            offset: byte_offset,
                            pointee: elem_ty.clone(),
                        });
                        addr
                    }
                    LValue::Indirect {
                        addr: base_addr, ..
                    } => {
                        let addr = self.vreg(Ty::Ref(Box::new(elem_ty.clone()), true));
                        self.emit(Inst::AddrOffset {
                            dst: addr,
                            base: *base_addr,
                            offset: byte_offset,
                            pointee: elem_ty.clone(),
                        });
                        addr
                    }
                };
                Some(LValue::Indirect {
                    addr,
                    offset: 0,
                    ty: elem_ty,
                    root,
                    root_name,
                })
            }
            Expr::Deref { expr, span } => {
                let inner = self.lower_expr(expr, None);
                let deref_ty = match &inner.ty {
                    Ty::Ref(pointee, _) => pointee.as_ref().clone(),
                    other => {
                        self.diags.push(Diag::error(
                            E_BAD_OPERANDS,
                            *span,
                            format!(
                                "dereference requires a reference, found `{}`",
                                self.type_name(other)
                            ),
                        ));
                        return None;
                    }
                };
                let root = self.lvalue_root_of_expr(expr);
                Some(LValue::Indirect {
                    addr: inner.vreg,
                    offset: 0,
                    ty: deref_ty,
                    root: root.unwrap_or(Slot(u32::MAX)),
                    root_name: "<deref>".to_string(),
                })
            }
            other => {
                self.diags.push(Diag::error(
                    E_BAD_OPERANDS,
                    other.span(),
                    "expression is not assignable".to_string(),
                ));
                None
            }
        }
    }

    pub(crate) fn lvalue_root_of_expr(&self, expr: &Expr) -> Option<Slot> {
        if let Expr::Ident { ident } = expr {
            return self.lookup(&ident.name).map(|binding| binding.slot);
        }
        None
    }

    // CEP:WHAT: Handles the cast for bounds step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn cast_for_bounds(&mut self, value: &Lowered) -> Vreg {
        if value.ty == Ty::U64 {
            return value.vreg;
        }
        let dst = self.vreg(Ty::U64);
        self.emit(Inst::Cast {
            dst,
            src: value.vreg,
            from: value.ty.clone(),
            to: Ty::U64,
        });
        dst
    }
}

/// Scalar-shaped types travel in vregs; aggregates travel as addresses.
pub fn is_scalar_shaped(ty: &Ty) -> bool {
    !matches!(ty, Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _))
}

fn lvalue_root_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident { ident } => Some(ident.name.clone()),
        Expr::Field { base, .. } | Expr::Index { base, .. } => lvalue_root_name(base),
        _ => None,
    }
}

impl<'a> FnBuilder<'a> {
    /// Loads the value of an lvalue into `dst` (used for reference-typed locals).
    // CEP:WHAT: Handles the read slot value step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub(crate) fn read_slot_value(&mut self, lvalue: &LValue, dst: Vreg) {
        match lvalue {
            LValue::Slot {
                slot, offset, ty, ..
            } => {
                if *offset == 0 {
                    self.emit(Inst::LoadSlot {
                        dst,
                        slot: *slot,
                        ty: ty.clone(),
                    });
                } else {
                    self.emit(Inst::LoadField {
                        dst,
                        slot: *slot,
                        offset: *offset,
                        ty: ty.clone(),
                    });
                }
            }
            LValue::Indirect {
                addr, offset, ty, ..
            } => {
                self.emit(Inst::LoadIndirect {
                    dst,
                    addr: *addr,
                    offset: *offset,
                    ty: ty.clone(),
                });
            }
        }
    }
}
