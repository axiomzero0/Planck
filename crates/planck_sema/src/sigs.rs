// CEP:FILE: crates/planck_sema/src/sigs.rs
// CEP:WHAT: Function signature collection and validation across top-level functions and impl methods.
// CEP:WHY: Calls and method calls must resolve against the complete signature table regardless of definition order; this pass also validates the main entry point contract.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Duplicate function names emit PLK-S0013; a missing or mistyped main emits PLK-S0008 or PLK-S0009; parameter type failures emit PLK-S0001 and yield a placeholder signature so later stages can proceed.
// CEP:ASSUMES: main must be top-level, parameterless, and return i32 exactly (design 2.1). Method names are qualified as `Type::method` in the signature table.
// CEP:COST: O(functions + parameters).
// CEP:EVIDENCE: cargo test -p planck_sema.
// CEP:SECURITY: none beyond type resolution diagnostics.
// CEP:PORTABILITY: Pure core.

#![forbid(unsafe_code)]

use crate::types::TypeTables;
use planck_ir::Ty;
use planck_parser::{Function, Item, Module, Param};
use planck_support::Diag;
use std::collections::BTreeMap;

pub const ERR_NO_MAIN: &str = "PLK-S0008";
pub const ERR_BAD_MAIN: &str = "PLK-S0009";

/// Fully resolved callable signature.
#[derive(Clone, Debug)]
pub struct FnSig {
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// Named type this method belongs to, if any.
    pub method_of: Option<String>,
    /// Index into the eventual program.funcs ordering.
    pub func_id: u32,
}

/// Collects signatures for all functions and impl methods, in deterministic program order:
/// top-level functions in declaration order first, then impl methods in declaration order.
/// Returns the ordered definitions so the lowering pass visits functions in the same order.
pub struct FnTable {
    pub sigs: BTreeMap<String, FnSig>,
    pub order: Vec<FunctionRef>,
}

#[derive(Clone, Debug)]
pub struct FunctionRef {
    pub qualified_name: String,
    /// Index into module items for top-level functions; methods reference (impl item, method index).
    pub location: FnLocation,
}

#[derive(Clone, Debug)]
pub enum FnLocation {
    TopLevel(usize),
    Method(usize, usize),
}

// CEP:WHAT: Handles the collect signatures step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn collect_signatures(module: &Module, tables: &TypeTables, diags: &mut Vec<Diag>) -> FnTable {
    let mut sigs: BTreeMap<String, FnSig> = BTreeMap::new();
    let mut order: Vec<FunctionRef> = Vec::new();
    let mut func_id: u32 = 0;

    let visit = |func: &Function,
                 item_idx: usize,
                 method_of: Option<String>,
                 diags: &mut Vec<Diag>,
                 sigs: &mut BTreeMap<String, FnSig>,
                 order: &mut Vec<FunctionRef>,
                 func_id: &mut u32| {
        if sigs.contains_key(&func.name.name) {
            diags.push(Diag::error(
                "PLK-S0013",
                func.name.span,
                format!("function `{}` is defined more than once", func.name.name),
            ));
            return;
        }
        let mut params = Vec::with_capacity(func.params.len());
        for param in &func.params {
            let resolved = if param.by_ref {
                match tables.resolve(&param.ty, diags) {
                    Some(inner) => Ty::Ref(Box::new(inner), param.ref_mut),
                    None => Ty::Unit,
                }
            } else {
                match tables.resolve(&param.ty, diags) {
                    Some(ty) => ty,
                    None => Ty::Unit,
                }
            };
            params.push(resolved);
        }
        let ret = tables.resolve(&func.ret, diags).unwrap_or(Ty::Unit);
        sigs.insert(
            func.name.name.clone(),
            FnSig {
                name: func.name.name.clone(),
                params,
                ret: ret.clone(),
                method_of,
                func_id: *func_id,
            },
        );
        order.push(FunctionRef {
            qualified_name: func.name.name.clone(),
            location: FnLocation::TopLevel(item_idx),
        });
        *func_id += 1;
    };

    for (item_idx, item) in module.items.iter().enumerate() {
        if let Item::Fn(func) = item {
            visit(
                func,
                item_idx,
                None,
                diags,
                &mut sigs,
                &mut order,
                &mut func_id,
            );
        }
    }
    for (impl_idx, item) in module.items.iter().enumerate() {
        if let Item::Impl(impl_block) = item {
            for (method_idx, func) in impl_block.funcs.iter().enumerate() {
                let qualified = format!("{}::{}", impl_block.self_ty.name, func.name.name);
                if sigs.contains_key(&qualified) {
                    diags.push(Diag::error(
                        "PLK-S0013",
                        func.name.span,
                        format!("function `{}` is defined more than once", qualified),
                    ));
                    continue;
                }
                let self_ty = tables
                    .struct_ids
                    .get(&impl_block.self_ty.name)
                    .map(|id| Ty::Struct(*id))
                    .or_else(|| {
                        tables
                            .enum_ids
                            .get(&impl_block.self_ty.name)
                            .map(|id| Ty::Enum(*id))
                    });
                let self_ty = match self_ty {
                    Some(ty) => ty,
                    None => {
                        diags.push(Diag::error(
                            "PLK-S0001",
                            impl_block.self_ty.span,
                            format!(
                                "impl target `{}` is not a known type",
                                impl_block.self_ty.name
                            ),
                        ));
                        continue;
                    }
                };
                let mut params = Vec::with_capacity(func.params.len());
                for param in &func.params {
                    if is_self_receiver(param) {
                        // Receiver position: `&self`, `&mut self`, or `self`.
                        params.push(if param.by_ref {
                            Ty::Ref(Box::new(self_ty.clone()), param.ref_mut)
                        } else {
                            self_ty.clone()
                        });
                        continue;
                    }
                    let resolved = if param.by_ref {
                        match tables.resolve(&param.ty, diags) {
                            Some(inner) => Ty::Ref(Box::new(inner), param.ref_mut),
                            None => Ty::Unit,
                        }
                    } else {
                        match tables.resolve(&param.ty, diags) {
                            Some(ty) => ty,
                            None => Ty::Unit,
                        }
                    };
                    params.push(resolved);
                }
                let ret = tables.resolve(&func.ret, diags).unwrap_or(Ty::Unit);
                sigs.insert(
                    qualified.clone(),
                    FnSig {
                        name: qualified.clone(),
                        params,
                        ret: ret.clone(),
                        method_of: Some(impl_block.self_ty.name.clone()),
                        func_id,
                    },
                );
                order.push(FunctionRef {
                    qualified_name: qualified,
                    location: FnLocation::Method(impl_idx, method_idx),
                });
                func_id += 1;
            }
        }
    }

    validate_main(module, &sigs, diags);
    FnTable { sigs, order }
}

// CEP:WHAT: Handles the validate main step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn validate_main(module: &Module, sigs: &BTreeMap<String, FnSig>, diags: &mut Vec<Diag>) {
    match sigs.get("main") {
        None => {
            let span = module
                .items
                .first()
                .map(|item| item.span())
                .unwrap_or_else(Span::new_zero);
            diags.push(Diag::error(
                ERR_NO_MAIN,
                span,
                "no `main` function found".to_string(),
            ));
        }
        Some(sig) => {
            if !sig.params.is_empty() || sig.ret != Ty::I32 || sig.method_of.is_some() {
                diags.push(Diag::error(
                    ERR_BAD_MAIN,
                    Span::new(0, 0),
                    "`main` must be a top-level `fn main() -> i32`".to_string(),
                ));
            }
        }
    }
}

use planck_support::Span;

/// True when the parameter is the method receiver written as `&self`, `&mut self`, or `self`.
pub fn is_self_receiver(param: &Param) -> bool {
    param.name.name == "self"
}
