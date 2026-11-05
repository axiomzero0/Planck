// CEP:FILE: crates/planck_sema/src/expr.rs
// CEP:WHAT: Expression lowering: literals, operators with short-circuit expansion, casts, calls, method calls, and aggregate construction.
// CEP:WHY: Expressions carry every typing rule of the core subset; isolating them keeps the statement lowering readable and gives each typing rule one home.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Reports PLK-S0002/S0003/S0004/S0006/S0007/S0016/S0017/S0018 as appropriate; malformed expressions lower to a unit placeholder and never panic.
// CEP:ASSUMES: Integer literals adopt the expected integer type or default to i32; float literals adopt f32 when expected, else f64; string literals have type &str (a reference to read-only data). Short-circuit operators lower to control flow with a result slot. Checked overflow applies to integer add/sub/mul and negation when the checked flag is set; division and remainder always trap on a zero divisor.
// CEP:COST: O(expression size); each node lowered once.
// CEP:EVIDENCE: cargo test -p planck_sema; end-to-end programs in crates/planckc/tests.
// CEP:SECURITY: Literal range checks reject out-of-range constants at compile time; bounds and overflow traps are emitted here and honored by codegen.
// CEP:PORTABILITY: Emits target-independent IR.

#![forbid(unsafe_code)]

use crate::lower::{is_scalar_shaped, FnBuilder, LValue, Lowered};
use crate::types::is_copy;
use planck_ir::{BinOp, CallArg, Callee, Const, Inst, Intrinsic, Term, TrapKind, Ty, UnOp};
use planck_parser::{BinOp as ABinOp, Expr, UnOp as AUnOp};
use planck_support::{Diag, Span};

#[allow(dead_code)]
const E_ARITY: &str = "PLK-S0004";
const E_BORROW: &str = "PLK-S0006";
const E_BAD_CAST: &str = "PLK-S0007";
const E_UNKNOWN_FIELD: &str = "PLK-S0011";
const E_UNKNOWN_VARIANT: &str = "PLK-S0012";
const E_BAD_OPERANDS: &str = "PLK-S0016";
const E_LITERAL_RANGE: &str = "PLK-S0021";
const E_UNKNOWN_METHOD: &str = "PLK-S0017";
const E_NOT_PRINTABLE: &str = "PLK-S0018";
const E_UNKNOWN_NAME: &str = "PLK-S0002";
const E_NOT_MUTABLE: &str = "PLK-S0005";

impl<'a> FnBuilder<'a> {
    pub fn lower_expr(&mut self, expr: &Expr, expect: Option<&Ty>) -> Lowered {
        match expr {
            Expr::Lit { lit, span } => self.lower_literal(lit, span, expect),
            Expr::Unit { .. } => Lowered {
                vreg: self.vreg(Ty::Unit),
                ty: Ty::Unit,
            },
            Expr::ArrayLit { elems, span } => self.lower_array_literal(elems, *span, expect),
            Expr::Ident { ident } => self.lower_ident_read(ident, expect),
            Expr::Unary {
                op,
                expr: inner,
                span,
            } => self.lower_unary(*op, inner, *span, expect),
            Expr::Binary { op, lhs, rhs, span } => self.lower_binary(*op, lhs, rhs, *span),
            Expr::Call { callee, args, span } => self.lower_call(callee, args, *span),
            Expr::MethodCall {
                receiver,
                method,
                args,
                span,
            } => self.lower_method(receiver, method, args, *span),
            Expr::Field { .. } | Expr::Index { .. } | Expr::Deref { .. } => {
                match self.resolve_lvalue(expr) {
                    Some(lvalue) => self.read_from_lvalue(&lvalue),
                    None => Lowered {
                        vreg: self.vreg(Ty::Unit),
                        ty: Ty::Unit,
                    },
                }
            }
            Expr::Cast {
                expr: inner,
                ty,
                span,
            } => self.lower_cast(inner, ty, *span),
            Expr::StructLit { name, fields, span } => {
                self.lower_struct_literal(name, fields, *span)
            }
            Expr::EnumLit {
                enum_name,
                variant,
                payload,
                span,
            } => self.lower_enum_literal(enum_name, variant, payload.as_deref(), *span),
            Expr::Borrow {
                mutable,
                expr: inner,
                span,
            } => self.lower_borrow(inner, *mutable, *span),
        }
    }

    // CEP:WHAT: Handles the lower array literal step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_array_literal(&mut self, elems: &[Expr], span: Span, expect: Option<&Ty>) -> Lowered {
        let (elem_ty, size) = match expect {
            Some(Ty::Array(elem, size)) => (elem.as_ref().clone(), *size),
            _ => {
                if elems.is_empty() {
                    self.diags.push(Diag::error(
                        E_BAD_OPERANDS,
                        span,
                        "array literal needs an expected type or at least one element".to_string(),
                    ));
                    return Lowered {
                        vreg: self.vreg(Ty::Unit),
                        ty: Ty::Unit,
                    };
                }
                // Infer from the first element; remaining elements must match.
                let first = self.lower_expr(&elems[0], None);
                let first_ty = first.ty.clone();
                return self.finish_array_literal(
                    elems,
                    first_ty,
                    elems.len() as u64,
                    span,
                    Some(first),
                );
            }
        };
        if elems.len() as u64 != size {
            self.diags.push(Diag::error(
                E_BAD_OPERANDS,
                span,
                format!(
                    "array literal has {} elements but type declares {}",
                    elems.len(),
                    size
                ),
            ));
        }
        self.finish_array_literal(elems, elem_ty, size, span, None)
    }

    // CEP:WHAT: Handles the finish array literal step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn finish_array_literal(
        &mut self,
        elems: &[Expr],
        elem_ty: Ty,
        size: u64,
        span: Span,
        first_lowered: Option<Lowered>,
    ) -> Lowered {
        let array_ty = Ty::Array(Box::new(elem_ty.clone()), size);
        let temp = self.slot(array_ty.clone(), "<array_tmp>");
        let elem_size = self.type_size(&elem_ty);
        let mut first = first_lowered;
        for (idx, elem_expr) in elems.iter().enumerate() {
            let value = match first.take() {
                Some(pre) => {
                    if pre.ty != elem_ty {
                        self.report_mismatch(elem_expr.span(), &elem_ty, &pre.ty);
                        continue;
                    }
                    pre
                }
                None => {
                    let value = self.lower_expr(elem_expr, Some(&elem_ty));
                    if value.ty != elem_ty {
                        self.report_mismatch(elem_expr.span(), &elem_ty, &value.ty);
                        continue;
                    }
                    value
                }
            };
            let offset = (idx as u32).saturating_mul(elem_size);
            match &elem_ty {
                Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _) => {
                    let base = self.vreg(Ty::Ref(Box::new(array_ty.clone()), true));
                    self.emit(Inst::AddrSlot {
                        dst: base,
                        slot: temp,
                        pointee: array_ty.clone(),
                        mutable: true,
                    });
                    self.emit(Inst::StoreIndirect {
                        addr: base,
                        offset,
                        src: value.vreg,
                        ty: Ty::Ref(Box::new(elem_ty.clone()), true),
                    });
                }
                _ => {
                    self.emit(Inst::StoreField {
                        slot: temp,
                        offset,
                        src: value.vreg,
                        ty: elem_ty.clone(),
                    });
                }
            }
        }
        let addr = self.vreg(Ty::Ref(Box::new(array_ty.clone()), false));
        self.emit(Inst::AddrSlot {
            dst: addr,
            slot: temp,
            pointee: array_ty.clone(),
            mutable: false,
        });
        let _ = span;
        Lowered {
            vreg: addr,
            ty: array_ty,
        }
    }

    // CEP:WHAT: Handles the lower literal step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_literal(
        &mut self,
        lit: &planck_lexer::Lit,
        span: &Span,
        expect: Option<&Ty>,
    ) -> Lowered {
        match lit {
            planck_lexer::Lit::Int(raw) => {
                let ty = match expect {
                    Some(ty) if ty.is_integer() => ty.clone(),
                    _ => Ty::I32,
                };
                if !int_fits(*raw, &ty) {
                    self.diags.push(Diag::error(
                        E_LITERAL_RANGE,
                        *span,
                        format!("literal `{}` does not fit in `{}`", raw, ty.name()),
                    ));
                }
                let vreg = self.vreg(ty.clone());
                self.emit(Inst::Const {
                    dst: vreg,
                    value: Const::Int(*raw as i128, ty.clone()),
                });
                Lowered { vreg, ty }
            }
            planck_lexer::Lit::Float(raw) => {
                let is_f32 = matches!(expect, Some(Ty::F32));
                let ty = if is_f32 { Ty::F32 } else { Ty::F64 };
                let vreg = self.vreg(ty.clone());
                self.emit(Inst::Const {
                    dst: vreg,
                    value: Const::Float(*raw, is_f32),
                });
                Lowered { vreg, ty }
            }
            planck_lexer::Lit::Bool(raw) => {
                let vreg = self.vreg(Ty::Bool);
                self.emit(Inst::Const {
                    dst: vreg,
                    value: Const::Bool(*raw),
                });
                Lowered { vreg, ty: Ty::Bool }
            }
            planck_lexer::Lit::Char(raw) => {
                let vreg = self.vreg(Ty::Char);
                self.emit(Inst::Const {
                    dst: vreg,
                    value: Const::Char(*raw),
                });
                Lowered { vreg, ty: Ty::Char }
            }
            planck_lexer::Lit::Str(raw) => {
                let id = self.intern_str(raw);
                let ty = Ty::Ref(Box::new(Ty::Str), false);
                let vreg = self.vreg(ty.clone());
                self.str_lens.insert(vreg.0, raw.len() as u32);
                self.emit(Inst::Const {
                    dst: vreg,
                    value: Const::Str(id),
                });
                Lowered { vreg, ty }
            }
        }
    }

    // CEP:WHAT: Handles the lower ident read step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_ident_read(&mut self, ident: &planck_parser::Ident, _expect: Option<&Ty>) -> Lowered {
        let binding = match self.lookup(&ident.name) {
            Some(binding) => binding,
            None => {
                self.diags.push(Diag::error(
                    E_UNKNOWN_NAME,
                    ident.span,
                    format!("unknown variable `{}`", ident.name),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
        };
        self.check_owner_use(binding.slot, &ident.name, false, ident.span);
        let lvalue = LValue::Slot {
            slot: binding.slot,
            offset: 0,
            ty: binding.ty.clone(),
            root: binding.slot,
            root_name: ident.name.clone(),
        };
        self.read_from_lvalue(&lvalue)
    }

    /// Reads a value out of an lvalue location.
    // CEP:WHAT: Handles the read from lvalue step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn read_from_lvalue(&mut self, lvalue: &LValue) -> Lowered {
        match lvalue {
            LValue::Slot {
                slot, offset, ty, ..
            } => match ty {
                Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _) => {
                    let addr = self.vreg(Ty::Ref(Box::new(ty.clone()), false));
                    self.emit(Inst::AddrSlot {
                        dst: addr,
                        slot: *slot,
                        pointee: ty.clone(),
                        mutable: false,
                    });
                    let value = self.aggregate_at(addr, *offset, ty);
                    Lowered {
                        vreg: value,
                        ty: ty.clone(),
                    }
                }
                _ => {
                    let dst = self.vreg(ty.clone());
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
                    Lowered {
                        vreg: dst,
                        ty: ty.clone(),
                    }
                }
            },
            LValue::Indirect {
                addr, offset, ty, ..
            } => match ty {
                Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _) => {
                    let value = self.aggregate_at(*addr, *offset, ty);
                    Lowered {
                        vreg: value,
                        ty: ty.clone(),
                    }
                }
                _ => {
                    let dst = self.vreg(ty.clone());
                    self.emit(Inst::LoadIndirect {
                        dst,
                        addr: *addr,
                        offset: *offset,
                        ty: ty.clone(),
                    });
                    Lowered {
                        vreg: dst,
                        ty: ty.clone(),
                    }
                }
            },
        }
    }

    /// Address-value for an aggregate at `base + offset`.
    // CEP:WHAT: Handles the aggregate at step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn aggregate_at(&mut self, base: planck_ir::Vreg, offset: u32, ty: &Ty) -> planck_ir::Vreg {
        if offset == 0 {
            return base;
        }
        let offset_vreg = self.vreg(Ty::U64);
        self.emit(Inst::Const {
            dst: offset_vreg,
            value: Const::Int(offset as i128, Ty::U64),
        });
        let addr = self.vreg(Ty::Ref(Box::new(ty.clone()), false));
        self.emit(Inst::AddrOffset {
            dst: addr,
            base,
            offset: offset_vreg,
            pointee: ty.clone(),
        });
        addr
    }

    // CEP:WHAT: Handles the lower unary step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_unary(&mut self, op: AUnOp, inner: &Expr, span: Span, expect: Option<&Ty>) -> Lowered {
        // Fold negative integer literals directly so range checks see the final value.
        if op == AUnOp::Neg {
            if let Expr::Lit {
                lit: planck_lexer::Lit::Int(raw),
                span: lit_span,
            } = inner
            {
                let ty = match expect {
                    Some(ty) if ty.is_integer() => ty.clone(),
                    _ => Ty::I32,
                };
                let negated: i128 = -(*raw as i128);
                if !int_range_contains(&ty, negated) {
                    self.diags.push(Diag::error(
                        E_LITERAL_RANGE,
                        *lit_span,
                        format!("literal `-{}` does not fit in `{}`", raw, ty.name()),
                    ));
                }
                let vreg = self.vreg(ty.clone());
                self.emit(Inst::Const {
                    dst: vreg,
                    value: Const::Int(negated, ty.clone()),
                });
                return Lowered { vreg, ty };
            }
        }
        let operand = match op {
            AUnOp::Neg => {
                let operand = self.lower_expr(inner, expect);
                match &operand.ty {
                    ty if ty.is_integer() || ty.is_float() => {}
                    other => {
                        self.diags.push(Diag::error(
                            E_BAD_OPERANDS,
                            span,
                            format!(
                                "negation requires a numeric operand, found `{}`",
                                self.type_name(other)
                            ),
                        ));
                        return Lowered {
                            vreg: self.vreg(Ty::Unit),
                            ty: Ty::Unit,
                        };
                    }
                }
                operand
            }
            AUnOp::Not => {
                let operand = self.lower_expr(inner, None);
                match &operand.ty {
                    Ty::Bool
                    | Ty::I8
                    | Ty::I16
                    | Ty::I32
                    | Ty::I64
                    | Ty::U8
                    | Ty::U16
                    | Ty::U32
                    | Ty::U64 => {}
                    other => {
                        self.diags.push(Diag::error(
                            E_BAD_OPERANDS,
                            span,
                            format!(
                                "`!` requires bool or integer, found `{}`",
                                self.type_name(other)
                            ),
                        ));
                        return Lowered {
                            vreg: self.vreg(Ty::Unit),
                            ty: Ty::Unit,
                        };
                    }
                }
                operand
            }
        };
        let checked = self.checked && operand.ty.is_integer();
        let dst = self.vreg(operand.ty.clone());
        self.emit(Inst::Un {
            op: match op {
                AUnOp::Neg => UnOp::Neg,
                AUnOp::Not => UnOp::Not,
            },
            dst,
            a: operand.vreg,
            ty: operand.ty.clone(),
            checked,
        });
        Lowered {
            vreg: dst,
            ty: operand.ty,
        }
    }

    // CEP:WHAT: Handles the lower binary step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_binary(&mut self, op: ABinOp, lhs: &Expr, rhs: &Expr, span: Span) -> Lowered {
        // Short-circuit logical operators lower to control flow with a result slot.
        if matches!(op, ABinOp::And | ABinOp::Or) {
            let lhs_value = self.lower_expr(lhs, Some(&Ty::Bool));
            if lhs_value.ty != Ty::Bool {
                self.diags.push(Diag::error(
                    E_BAD_OPERANDS,
                    lhs.span(),
                    "logical operators require bool operands".to_string(),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
            let result_slot = self.slot(Ty::Bool, "<short_circuit>");
            self.emit(Inst::StoreSlot {
                slot: result_slot,
                src: lhs_value.vreg,
                ty: Ty::Bool,
            });
            let rhs_blk = self.block();
            let end_blk = self.block();
            let (then_blk, else_blk) = match op {
                ABinOp::And => (rhs_blk, end_blk),
                _ => (end_blk, rhs_blk),
            };
            self.terminate_and_start(Term::BrCond {
                cond: lhs_value.vreg,
                then_blk,
                else_blk,
            });
            self.cur = rhs_blk;
            let rhs_value = self.lower_expr(rhs, Some(&Ty::Bool));
            if rhs_value.ty != Ty::Bool {
                self.diags.push(Diag::error(
                    E_BAD_OPERANDS,
                    rhs.span(),
                    "logical operators require bool operands".to_string(),
                ));
            }
            self.emit(Inst::StoreSlot {
                slot: result_slot,
                src: rhs_value.vreg,
                ty: Ty::Bool,
            });
            self.terminate_and_start(Term::Br(end_blk));
            self.cur = end_blk;
            let dst = self.vreg(Ty::Bool);
            self.emit(Inst::LoadSlot {
                dst,
                slot: result_slot,
                ty: Ty::Bool,
            });
            return Lowered {
                vreg: dst,
                ty: Ty::Bool,
            };
        }

        let lhs_value = self.lower_expr(lhs, None);
        let rhs_value = self.lower_expr(rhs, Some(&lhs_value.ty));

        let is_comparison = matches!(
            op,
            ABinOp::Eq | ABinOp::Ne | ABinOp::Lt | ABinOp::Le | ABinOp::Gt | ABinOp::Ge
        );
        let is_logical = matches!(op, ABinOp::And | ABinOp::Or);

        if lhs_value.ty != rhs_value.ty {
            self.report_mismatch(span, &lhs_value.ty, &rhs_value.ty);
            return Lowered {
                vreg: self.vreg(Ty::Unit),
                ty: Ty::Unit,
            };
        }
        let operand_ty = lhs_value.ty.clone();
        let valid = if is_comparison {
            operand_ty.is_integer()
                || operand_ty.is_float()
                || operand_ty == Ty::Bool
                || operand_ty == Ty::Char
        } else if is_logical {
            operand_ty == Ty::Bool
        } else {
            operand_ty.is_integer() || operand_ty.is_float()
        };
        if !valid {
            self.diags.push(Diag::error(
                E_BAD_OPERANDS,
                span,
                format!(
                    "operator `{}` is not defined for `{}`",
                    op.symbol(),
                    self.type_name(&operand_ty)
                ),
            ));
            return Lowered {
                vreg: self.vreg(Ty::Unit),
                ty: Ty::Unit,
            };
        }

        let ir_op = match op {
            ABinOp::Add => BinOp::Add,
            ABinOp::Sub => BinOp::Sub,
            ABinOp::Mul => BinOp::Mul,
            ABinOp::Div => BinOp::Div,
            ABinOp::Rem => BinOp::Rem,
            ABinOp::Eq => BinOp::Eq,
            ABinOp::Ne => BinOp::Ne,
            ABinOp::Lt => BinOp::Lt,
            ABinOp::Le => BinOp::Le,
            ABinOp::Gt => BinOp::Gt,
            ABinOp::Ge => BinOp::Ge,
            ABinOp::And | ABinOp::Or => unreachable!("handled above"),
            ABinOp::BitAnd => BinOp::BitAnd,
            ABinOp::BitOr => BinOp::BitOr,
            ABinOp::BitXor => BinOp::BitXor,
            ABinOp::Shl => BinOp::Shl,
            ABinOp::Shr => BinOp::Shr,
        };

        // Division and remainder always trap on a zero divisor (deterministic behavior).
        if matches!(ir_op, BinOp::Div | BinOp::Rem) && operand_ty.is_integer() {
            let zero = self.vreg(operand_ty.clone());
            self.emit(Inst::Const {
                dst: zero,
                value: Const::Int(0, operand_ty.clone()),
            });
            let is_zero = self.vreg(Ty::Bool);
            self.emit(Inst::Bin {
                op: BinOp::Eq,
                dst: is_zero,
                a: rhs_value.vreg,
                b: zero,
                ty: operand_ty.clone(),
                checked: false,
            });
            self.emit(Inst::TrapIf {
                cond: is_zero,
                trap: TrapKind::DivZero,
            });
        }

        let result_ty = if is_comparison {
            Ty::Bool
        } else {
            operand_ty.clone()
        };
        let checked = self.checked
            && operand_ty.is_integer()
            && matches!(ir_op, BinOp::Add | BinOp::Sub | BinOp::Mul);
        let dst = self.vreg(result_ty.clone());
        self.emit(Inst::Bin {
            op: ir_op,
            dst,
            a: lhs_value.vreg,
            b: rhs_value.vreg,
            ty: operand_ty,
            checked,
        });
        Lowered {
            vreg: dst,
            ty: result_ty,
        }
    }

    // CEP:WHAT: Handles the lower cast step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_cast(&mut self, inner: &Expr, to: &planck_parser::TypeExpr, span: Span) -> Lowered {
        let operand = self.lower_expr(inner, None);
        let to_ty = match self.tables.resolve(to, self.diags) {
            Some(ty) => ty,
            None => {
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                }
            }
        };
        let from_ty = operand.ty.clone();
        let allowed = match (&from_ty, &to_ty) {
            (a, b) if a.is_integer() && b.is_integer() => true,
            (a, b) if a.is_integer() && b.is_float() => true,
            (a, b) if a.is_float() && b.is_integer() => true,
            (a, b) if a.is_float() && b.is_float() => true,
            (Ty::Char, b) if b.is_integer() => true,
            (a, Ty::Char) if a.is_integer() => true,
            (Ty::Bool, b) if b.is_integer() => true,
            _ => false,
        };
        if !allowed {
            self.diags.push(Diag::error(
                E_BAD_CAST,
                span,
                format!(
                    "cannot cast `{}` to `{}`",
                    self.type_name(&from_ty),
                    self.type_name(&to_ty)
                ),
            ));
            return Lowered {
                vreg: self.vreg(Ty::Unit),
                ty: Ty::Unit,
            };
        }
        if from_ty == to_ty {
            return operand;
        }
        let dst = self.vreg(to_ty.clone());
        self.emit(Inst::Cast {
            dst,
            src: operand.vreg,
            from: from_ty,
            to: to_ty.clone(),
        });
        Lowered {
            vreg: dst,
            ty: to_ty,
        }
    }

    // CEP:WHAT: Handles the lower struct literal step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_struct_literal(
        &mut self,
        name: &planck_parser::Ident,
        fields: &[(planck_parser::Ident, Expr)],
        span: Span,
    ) -> Lowered {
        let struct_id = match self.tables.struct_ids.get(&name.name) {
            Some(id) => *id,
            None => {
                self.diags.push(Diag::error(
                    E_UNKNOWN_NAME,
                    name.span,
                    format!("unknown struct `{}`", name.name),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
        };
        let info = self.tables.struct_ty(struct_id).clone();
        // Duplicate field check.
        let mut seen: Vec<&str> = Vec::new();
        for (field, _) in fields {
            if seen.contains(&field.name.as_str()) {
                self.diags.push(Diag::error(
                    E_UNKNOWN_FIELD,
                    field.span,
                    format!("field `{}` specified more than once", field.name),
                ));
            }
            seen.push(&field.name);
        }
        // Missing field check.
        for (field_name, _, _) in &info.fields {
            if !seen.contains(&field_name.as_str()) {
                self.diags.push(Diag::error(
                    E_UNKNOWN_FIELD,
                    span,
                    format!("struct `{}` is missing field `{}`", info.name, field_name),
                ));
            }
        }
        let temp = self.slot(Ty::Struct(struct_id), "<struct_tmp>");
        for (field, value_expr) in fields {
            let (_idx, field_ty, offset) = match self.struct_field_index(struct_id, &field.name) {
                Some(found) => found,
                None => continue, // already reported
            };
            let value = self.lower_expr(value_expr, Some(&field_ty));
            if value.ty != field_ty {
                self.report_mismatch(value_expr.span(), &field_ty, &value.ty);
                continue;
            }
            match &field_ty {
                Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _) => {
                    let base = self.vreg(Ty::Ref(Box::new(Ty::Struct(struct_id)), true));
                    self.emit(Inst::AddrSlot {
                        dst: base,
                        slot: temp,
                        pointee: Ty::Struct(struct_id),
                        mutable: true,
                    });
                    self.emit(Inst::StoreIndirect {
                        addr: base,
                        offset,
                        src: value.vreg,
                        ty: Ty::Ref(Box::new(field_ty.clone()), true),
                    });
                }
                _ => {
                    self.emit(Inst::StoreField {
                        slot: temp,
                        offset,
                        src: value.vreg,
                        ty: field_ty,
                    });
                }
            }
        }
        let addr = self.vreg(Ty::Ref(Box::new(Ty::Struct(struct_id)), false));
        self.emit(Inst::AddrSlot {
            dst: addr,
            slot: temp,
            pointee: Ty::Struct(struct_id),
            mutable: false,
        });
        Lowered {
            vreg: addr,
            ty: Ty::Struct(struct_id),
        }
    }

    // CEP:WHAT: Handles the lower enum literal step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_enum_literal(
        &mut self,
        enum_name: &planck_parser::Ident,
        variant: &planck_parser::Ident,
        payload: Option<&Expr>,
        span: Span,
    ) -> Lowered {
        let enum_id = match self.tables.enum_ids.get(&enum_name.name) {
            Some(id) => *id,
            None => {
                self.diags.push(Diag::error(
                    E_UNKNOWN_NAME,
                    enum_name.span,
                    format!("unknown enum `{}`", enum_name.name),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
        };
        let (variant_idx, payload_ty) = match self.enum_variant_index(enum_id, &variant.name) {
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
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
        };
        match (&payload_ty, payload) {
            (Some(_), None) => {
                self.diags.push(Diag::error(
                    E_ARITY,
                    span,
                    format!("variant `{}` requires a payload", variant.name),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
            (None, Some(expr)) => {
                self.diags.push(Diag::error(
                    E_ARITY,
                    expr.span(),
                    format!("variant `{}` has no payload", variant.name),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
            _ => {}
        }
        let temp = self.slot(Ty::Enum(enum_id), "<enum_tmp>");
        let tag = self.vreg(Ty::U32);
        self.emit(Inst::Const {
            dst: tag,
            value: Const::Int(variant_idx as i128, Ty::U32),
        });
        self.emit(Inst::StoreField {
            slot: temp,
            offset: 0,
            src: tag,
            ty: Ty::U32,
        });
        if let (Some(payload_expr), Some(pty)) = (payload, &payload_ty) {
            let payload_offset = self.tables.enum_ty(enum_id).payload_offset;
            let value = self.lower_expr(payload_expr, Some(pty));
            if value.ty != *pty {
                self.report_mismatch(payload_expr.span(), pty, &value.ty);
            } else {
                match pty {
                    Ty::Struct(_) | Ty::Enum(_) | Ty::Array(_, _) => {
                        let base = self.vreg(Ty::Ref(Box::new(Ty::Enum(enum_id)), true));
                        self.emit(Inst::AddrSlot {
                            dst: base,
                            slot: temp,
                            pointee: Ty::Enum(enum_id),
                            mutable: true,
                        });
                        self.emit(Inst::StoreIndirect {
                            addr: base,
                            offset: payload_offset,
                            src: value.vreg,
                            ty: Ty::Ref(Box::new(pty.clone()), true),
                        });
                    }
                    _ => {
                        self.emit(Inst::StoreField {
                            slot: temp,
                            offset: payload_offset,
                            src: value.vreg,
                            ty: pty.clone(),
                        });
                    }
                }
            }
        }
        let addr = self.vreg(Ty::Ref(Box::new(Ty::Enum(enum_id)), false));
        self.emit(Inst::AddrSlot {
            dst: addr,
            slot: temp,
            pointee: Ty::Enum(enum_id),
            mutable: false,
        });
        Lowered {
            vreg: addr,
            ty: Ty::Enum(enum_id),
        }
    }

    // CEP:WHAT: Handles the lower borrow step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_borrow(&mut self, inner: &Expr, mutable: bool, span: Span) -> Lowered {
        let lvalue = match self.resolve_lvalue(inner) {
            Some(lvalue) => lvalue,
            None => {
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                }
            }
        };
        let root = lvalue.root();
        let root_name = lvalue.root_name().to_string();
        if mutable {
            let root_mutable = self
                .lookup(&root_name)
                .map(|binding| binding.mutable)
                .unwrap_or(false);
            if !root_mutable {
                self.diags.push(Diag::error(
                    E_NOT_MUTABLE,
                    span,
                    format!(
                        "cannot borrow `{}` as mutable: not declared as mutable",
                        root_name
                    ),
                ));
            }
        }
        self.create_borrow(root, &root_name, mutable, false, span);
        let pointee = lvalue.ty().clone();
        match lvalue {
            LValue::Slot { slot, .. } => {
                let addr = self.vreg(Ty::Ref(Box::new(pointee.clone()), mutable));
                self.emit(Inst::AddrSlot {
                    dst: addr,
                    slot,
                    pointee: pointee.clone(),
                    mutable,
                });
                Lowered {
                    vreg: addr,
                    ty: Ty::Ref(Box::new(pointee), mutable),
                }
            }
            LValue::Indirect {
                addr, offset, ty, ..
            } => {
                if offset == 0 {
                    Lowered {
                        vreg: addr,
                        ty: Ty::Ref(Box::new(ty.clone()), mutable),
                    }
                } else {
                    let reborrowed = self.aggregate_at(addr, offset, &ty);
                    Lowered {
                        vreg: reborrowed,
                        ty: Ty::Ref(Box::new(ty), mutable),
                    }
                }
            }
        }
    }

    // CEP:WHAT: Handles the lower call step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_call(&mut self, callee: &planck_parser::Ident, args: &[Expr], span: Span) -> Lowered {
        // Built-in print, resolved by argument type.
        if callee.name == "print" && !self.sigs.contains_key("print") {
            return self.lower_print(args, span);
        }
        let sig = match self.sigs.get(&callee.name) {
            Some(sig) => sig.clone(),
            None => {
                self.diags.push(Diag::error(
                    E_UNKNOWN_NAME,
                    callee.span,
                    format!("unknown function `{}`", callee.name),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
        };
        self.emit_call(&sig, args, span)
    }

    // CEP:WHAT: Handles the lower print step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_print(&mut self, args: &[Expr], span: Span) -> Lowered {
        if args.len() != 1 {
            self.diags.push(Diag::error(
                E_ARITY,
                span,
                format!("`print` takes exactly one argument, found {}", args.len()),
            ));
            return Lowered {
                vreg: self.vreg(Ty::Unit),
                ty: Ty::Unit,
            };
        }
        let value = self.lower_expr(&args[0], None);
        let intrinsic = match &value.ty {
            Ty::Ref(inner, _) if matches!(inner.as_ref(), Ty::Str) => Intrinsic::PrintStr,
            Ty::I8
            | Ty::I16
            | Ty::I32
            | Ty::I64
            | Ty::U8
            | Ty::U16
            | Ty::U32
            | Ty::U64
            | Ty::F32
            | Ty::F64
            | Ty::Bool
            | Ty::Char => Intrinsic::Print,
            other => {
                self.diags.push(Diag::error(
                    E_NOT_PRINTABLE,
                    span,
                    format!("`print` cannot display `{}`", self.type_name(other)),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
        };
        let args = if intrinsic == Intrinsic::PrintStr {
            // String printing passes (pointer, byte length); the length is a compile-time
            // constant from the rodata table.
            let len = self.str_lens.get(&value.vreg.0).copied().unwrap_or(0);
            let len_vreg = self.vreg(Ty::U64);
            self.emit(Inst::Const {
                dst: len_vreg,
                value: planck_ir::Const::Int(len as i128, Ty::U64),
            });
            vec![
                CallArg {
                    value: value.vreg,
                    ty: value.ty.clone(),
                    by_ref: false,
                },
                CallArg {
                    value: len_vreg,
                    ty: Ty::U64,
                    by_ref: false,
                },
            ]
        } else {
            vec![CallArg {
                value: value.vreg,
                ty: value.ty.clone(),
                by_ref: false,
            }]
        };
        self.emit(Inst::Call {
            dst: None,
            sret: None,
            func: Callee::Intrinsic(intrinsic),
            args,
            ret: Ty::Unit,
        });
        Lowered {
            vreg: self.vreg(Ty::Unit),
            ty: Ty::Unit,
        }
    }

    // CEP:WHAT: Handles the lower method step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_method(
        &mut self,
        receiver: &Expr,
        method: &planck_parser::Ident,
        args: &[Expr],
        span: Span,
    ) -> Lowered {
        // Resolve the receiver to an address where possible.
        let receiver_lvalue = self.resolve_lvalue(receiver);
        let receiver_ty = match &receiver_lvalue {
            Some(lvalue) => lvalue.ty().clone(),
            None => match self.peek_expr_type(receiver) {
                Some(ty) => ty,
                None => {
                    return Lowered {
                        vreg: self.vreg(Ty::Unit),
                        ty: Ty::Unit,
                    }
                }
            },
        };
        let type_name = self.type_name(&receiver_ty);
        let qualified = format!("{}::{}", type_name, method.name);
        let sig = match self.sigs.get(&qualified) {
            Some(sig) => sig.clone(),
            None => {
                self.diags.push(Diag::error(
                    E_UNKNOWN_METHOD,
                    method.span,
                    format!("type `{}` has no method `{}`", type_name, method.name),
                ));
                return Lowered {
                    vreg: self.vreg(Ty::Unit),
                    ty: Ty::Unit,
                };
            }
        };
        // Receiver value/address.
        let receiver_param = sig.params.first().cloned().unwrap_or(Ty::Unit);
        let receiver_arg = match (&receiver_lvalue, &receiver_param) {
            (Some(lvalue), Ty::Ref(pointee, mutable)) => {
                // Method borrows the receiver.
                let root = lvalue.root();
                let root_name = lvalue.root_name().to_string();
                if *mutable {
                    let root_mutable = self
                        .lookup(&root_name)
                        .map(|binding| binding.mutable)
                        .unwrap_or(false);
                    if !root_mutable {
                        self.diags.push(Diag::error(
                            E_NOT_MUTABLE,
                            span,
                            format!(
                                "cannot borrow `{}` as mutable: not declared as mutable",
                                root_name
                            ),
                        ));
                    }
                }
                self.create_borrow(root, &root_name, *mutable, false, span);
                match lvalue {
                    LValue::Slot { slot, .. } => {
                        let addr = self.vreg(Ty::Ref(Box::new(pointee.as_ref().clone()), *mutable));
                        self.emit(Inst::AddrSlot {
                            dst: addr,
                            slot: *slot,
                            pointee: pointee.as_ref().clone(),
                            mutable: *mutable,
                        });
                        addr
                    }
                    LValue::Indirect {
                        addr, offset, ty, ..
                    } => {
                        if *offset == 0 {
                            *addr
                        } else {
                            self.aggregate_at(*addr, *offset, ty)
                        }
                    }
                }
            }
            (Some(lvalue), expected) if !is_scalar_shaped(expected) => {
                // By-value aggregate receiver: copy into a temporary and pass its address.
                let value = self.read_from_lvalue(lvalue);
                let temp = self.slot(expected.clone(), "<recv_tmp>");
                self.emit(Inst::StoreStructFromAddr {
                    dst_slot: temp,
                    src_addr: value.vreg,
                    ty: expected.clone(),
                });
                let addr = self.vreg(Ty::Ref(Box::new(expected.clone()), false));
                self.emit(Inst::AddrSlot {
                    dst: addr,
                    slot: temp,
                    pointee: expected.clone(),
                    mutable: false,
                });
                addr
            }
            (Some(lvalue), expected) => {
                let value = self.read_from_lvalue(lvalue);
                if value.ty != *expected {
                    self.report_mismatch(receiver.span(), expected, &value.ty);
                }
                value.vreg
            }
            (None, expected) => {
                // Receiver is a computed expression.
                let value = self.lower_expr(receiver, Some(expected));
                if !is_scalar_shaped(expected) {
                    let temp = self.slot(expected.clone(), "<recv_tmp>");
                    self.emit(Inst::StoreStructFromAddr {
                        dst_slot: temp,
                        src_addr: value.vreg,
                        ty: expected.clone(),
                    });
                    let addr = self.vreg(Ty::Ref(Box::new(expected.clone()), false));
                    self.emit(Inst::AddrSlot {
                        dst: addr,
                        slot: temp,
                        pointee: expected.clone(),
                        mutable: false,
                    });
                    addr
                } else {
                    value.vreg
                }
            }
        };

        // Remaining arguments.
        let mut call_args = vec![CallArg {
            value: receiver_arg,
            ty: receiver_param.clone(),
            by_ref: !is_scalar_shaped(&receiver_param) || matches!(receiver_param, Ty::Ref(_, _)),
        }];
        for (param_idx, arg_expr) in args.iter().enumerate() {
            let param_ty = match sig.params.get(param_idx + 1) {
                Some(ty) => ty.clone(),
                None => {
                    self.diags.push(Diag::error(
                        E_ARITY,
                        span,
                        format!(
                            "`{}` expects {} arguments, found {}",
                            sig.name,
                            sig.params.len().saturating_sub(1),
                            args.len()
                        ),
                    ));
                    return Lowered {
                        vreg: self.vreg(Ty::Unit),
                        ty: Ty::Unit,
                    };
                }
            };
            let arg = self.lower_call_arg(arg_expr, &param_ty);
            call_args.push(arg);
        }
        if args.len() + 1 != sig.params.len() {
            self.diags.push(Diag::error(
                E_ARITY,
                span,
                format!(
                    "`{}` expects {} arguments, found {}",
                    sig.name,
                    sig.params.len().saturating_sub(1),
                    args.len()
                ),
            ));
        }

        self.emit_call_inst(&sig, call_args, span)
    }

    // CEP:WHAT: Handles the peek expr type step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn peek_expr_type(&mut self, expr: &Expr) -> Option<Ty> {
        match expr {
            Expr::Ident { ident } => self.lookup(&ident.name).map(|b| b.ty),
            Expr::Field { base, field, span } => {
                let base_ty = self.peek_expr_type(base)?;
                let struct_id = match base_ty {
                    Ty::Struct(id) => id,
                    _ => {
                        let _ = span;
                        return None;
                    }
                };
                self.struct_field_index(struct_id, &field.name)
                    .map(|(_, ty, _)| ty)
            }
            _ => None,
        }
    }

    // CEP:WHAT: Handles the lower call arg step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn lower_call_arg(&mut self, arg_expr: &Expr, param_ty: &Ty) -> CallArg {
        match param_ty {
            Ty::Ref(pointee, mutable) => {
                // Reference parameter: the argument must be a borrow expression.
                match arg_expr {
                    Expr::Borrow {
                        mutable: borrow_mut,
                        expr: borrowed,
                        ..
                    } => {
                        if borrow_mut != mutable {
                            self.diags.push(Diag::error(
                                E_BORROW,
                                arg_expr.span(),
                                format!(
                                    "parameter expects {} reference",
                                    if *mutable {
                                        "a mutable"
                                    } else {
                                        "an immutable"
                                    }
                                ),
                            ));
                        }
                        let lvalue = match self.resolve_lvalue(borrowed) {
                            Some(lvalue) => lvalue,
                            None => {
                                return CallArg {
                                    value: self.vreg(Ty::Unit),
                                    ty: param_ty.clone(),
                                    by_ref: false,
                                }
                            }
                        };
                        let root = lvalue.root();
                        let root_name = lvalue.root_name().to_string();
                        if *mutable {
                            let root_mutable = self
                                .lookup(&root_name)
                                .map(|binding| binding.mutable)
                                .unwrap_or(false);
                            if !root_mutable {
                                self.diags.push(Diag::error(
                                    E_NOT_MUTABLE,
                                    arg_expr.span(),
                                    format!(
                                        "cannot borrow `{}` as mutable: not declared as mutable",
                                        root_name
                                    ),
                                ));
                            }
                        }
                        self.create_borrow(root, &root_name, *mutable, false, arg_expr.span());
                        let addr = match lvalue {
                            LValue::Slot { slot, .. } => {
                                let addr = self
                                    .vreg(Ty::Ref(Box::new(pointee.as_ref().clone()), *mutable));
                                self.emit(Inst::AddrSlot {
                                    dst: addr,
                                    slot,
                                    pointee: pointee.as_ref().clone(),
                                    mutable: *mutable,
                                });
                                addr
                            }
                            LValue::Indirect {
                                addr, offset, ty, ..
                            } => {
                                if offset == 0 {
                                    addr
                                } else {
                                    self.aggregate_at(addr, offset, &ty)
                                }
                            }
                        };
                        CallArg {
                            value: addr,
                            ty: param_ty.clone(),
                            by_ref: false,
                        }
                    }
                    other => {
                        // `&str` parameters accept string literals directly.
                        if matches!(param_ty, Ty::Ref(inner, false) if matches!(inner.as_ref(), Ty::Str))
                        {
                            let value = self.lower_expr(other, None);
                            return CallArg {
                                value: value.vreg,
                                ty: param_ty.clone(),
                                by_ref: false,
                            };
                        }
                        self.diags.push(Diag::error(
                            E_BORROW,
                            arg_expr.span(),
                            "reference parameter requires a `&` or `&mut` argument".to_string(),
                        ));
                        CallArg {
                            value: self.vreg(Ty::Unit),
                            ty: param_ty.clone(),
                            by_ref: false,
                        }
                    }
                }
            }
            expected if !is_scalar_shaped(expected) => {
                // Aggregate by value: caller copy.
                let value = self.lower_expr(arg_expr, Some(expected));
                if value.ty != *expected {
                    self.report_mismatch(arg_expr.span(), expected, &value.ty);
                }
                let temp = self.slot(expected.clone(), "<arg_tmp>");
                self.emit(Inst::StoreStructFromAddr {
                    dst_slot: temp,
                    src_addr: value.vreg,
                    ty: expected.clone(),
                });
                let addr = self.vreg(Ty::Ref(Box::new(expected.clone()), false));
                self.emit(Inst::AddrSlot {
                    dst: addr,
                    slot: temp,
                    pointee: expected.clone(),
                    mutable: false,
                });
                CallArg {
                    value: addr,
                    ty: expected.clone(),
                    by_ref: true,
                }
            }
            expected => {
                let value = self.lower_expr(arg_expr, Some(expected));
                if value.ty != *expected {
                    self.report_mismatch(arg_expr.span(), expected, &value.ty);
                }
                CallArg {
                    value: value.vreg,
                    ty: value.ty,
                    by_ref: false,
                }
            }
        }
    }

    // CEP:WHAT: Handles the emit call step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn emit_call(&mut self, sig: &crate::sigs::FnSig, args: &[Expr], span: Span) -> Lowered {
        if args.len() != sig.params.len() {
            self.diags.push(Diag::error(
                E_ARITY,
                span,
                format!(
                    "`{}` expects {} arguments, found {}",
                    sig.name,
                    sig.params.len(),
                    args.len()
                ),
            ));
        }
        let mut call_args = Vec::with_capacity(args.len());
        for (arg_expr, param_ty) in args.iter().zip(sig.params.iter()) {
            call_args.push(self.lower_call_arg(arg_expr, param_ty));
        }
        self.emit_call_inst(sig, call_args, span)
    }

    /// Emits the Call inst (with sret for aggregate returns) and returns the lowered result.
    // CEP:WHAT: Handles the emit call inst step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn emit_call_inst(
        &mut self,
        sig: &crate::sigs::FnSig,
        call_args: Vec<CallArg>,
        _span: Span,
    ) -> Lowered {
        let ret = sig.ret.clone();
        let sret = if !matches!(ret, Ty::Unit) && !is_scalar_shaped(&ret) {
            let slot = self.slot(ret.clone(), "<ret_tmp>");
            let addr = self.vreg(Ty::Ref(Box::new(ret.clone()), false));
            self.emit(Inst::AddrSlot {
                dst: addr,
                slot,
                pointee: ret.clone(),
                mutable: false,
            });
            Some(addr)
        } else {
            None
        };
        let dst = if matches!(ret, Ty::Unit) {
            None
        } else if is_scalar_shaped(&ret) {
            Some(self.vreg(ret.clone()))
        } else {
            None
        };
        self.emit(Inst::Call {
            dst,
            sret,
            func: Callee::Func(planck_ir::FuncId(sig.func_id)),
            args: call_args,
            ret: ret.clone(),
        });
        match (dst, sret) {
            (Some(dst), _) => Lowered { vreg: dst, ty: ret },
            (None, Some(sret)) => Lowered {
                vreg: sret,
                ty: ret,
            },
            (None, None) => Lowered {
                vreg: self.vreg(Ty::Unit),
                ty: Ty::Unit,
            },
        }
    }
}

/// Whether a non-negative u64 literal fits in the given integer type.
fn int_fits(raw: u64, ty: &Ty) -> bool {
    let value = raw as i128;
    match ty {
        Ty::I8 => (i8::MIN as i128..=i8::MAX as i128).contains(&value),
        Ty::I16 => (i16::MIN as i128..=i16::MAX as i128).contains(&value),
        Ty::I32 => (i32::MIN as i128..=i32::MAX as i128).contains(&value),
        Ty::I64 => (i64::MIN as i128..=i64::MAX as i128).contains(&value),
        Ty::U8 => value <= u8::MAX as i128,
        Ty::U16 => value <= u16::MAX as i128,
        Ty::U32 => value <= u32::MAX as i128,
        Ty::U64 => true,
        _ => true,
    }
}

fn int_range_contains(ty: &Ty, value: i128) -> bool {
    match ty {
        Ty::I8 => (i8::MIN as i128..=i8::MAX as i128).contains(&value),
        Ty::I16 => (i16::MIN as i128..=i16::MAX as i128).contains(&value),
        Ty::I32 => (i32::MIN as i128..=i32::MAX as i128).contains(&value),
        Ty::I64 => (i64::MIN as i128..=i64::MAX as i128).contains(&value),
        Ty::U8 => (0..=u8::MAX as i128).contains(&value),
        Ty::U16 => (0..=u16::MAX as i128).contains(&value),
        Ty::U32 => (0..=u32::MAX as i128).contains(&value),
        Ty::U64 => (0..=u64::MAX as i128).contains(&value),
        _ => true,
    }
}

// is_copy is re-exported for future move-semantics engagement (PLK-010).
#[allow(dead_code)]
fn _copy_check(ty: &Ty) -> bool {
    is_copy(ty)
}
