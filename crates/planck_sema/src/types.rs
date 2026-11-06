// CEP:FILE: crates/planck_sema/src/types.rs
// CEP:WHAT: Type resolution, aggregate layout computation with memoization and cycle detection, and Copy classification.
// CEP:WHY: Layout decisions (field offsets, enum tag placement) and the Copy classification drive every load, store, and call lowering decision; one authoritative layout per type prevents frontend/backend divergence.
// CEP:CLASS: CEP-1
// CEP:STATUS: complete
// CEP:FAILURE: Unknown type names emit PLK-S0001; duplicate type names emit PLK-S0013; value-recursive aggregates emit PLK-S0019. On any resolution failure the affected field resolves to a placeholder unit and the driver halts before codegen.
// CEP:ASSUMES: Layout is declaration-order with natural alignment; enum tag is u32 at offset 0; payload offset is aligned to payload alignment. Structs may contain structs and arrays by value; cycles through value fields are rejected.
// CEP:COST: O(total fields) layout with memoization; resolution O(log T) per name via ordered tables (deterministic iteration).
// CEP:EVIDENCE: cargo test -p planck_sema.
// CEP:SECURITY: Array sizes come from source literals and are clamped to u32::MAX to keep offset arithmetic overflow-free.
// CEP:PORTABILITY: Layout follows x86-64 System V natural alignment; planck_codegen matches it.

#![forbid(unsafe_code)]

use planck_ir::{align_up, scalar_align, EnumId, EnumInfo, StructId, StructInfo, Ty};
use planck_parser::{EnumDef, Item, Module, StructDef, TypeExpr};
use planck_support::{Diag, Span};
use std::collections::{BTreeMap, BTreeSet};

type RawStructDefs = Vec<(String, Vec<(String, TypeExpr)>)>;
type RawEnumDefs = Vec<(String, Vec<(String, Option<TypeExpr>)>)>;

pub const ERR_UNKNOWN_TYPE: &str = "PLK-S0001";
pub const ERR_DUPLICATE_DEF: &str = "PLK-S0013";
pub const ERR_RECURSIVE_TYPE: &str = "PLK-S0019";

/// Resolved type tables shared by all lowering passes.
pub struct TypeTables {
    pub struct_ids: BTreeMap<String, StructId>,
    pub enum_ids: BTreeMap<String, EnumId>,
    pub structs: Vec<StructInfo>,
    pub enums: Vec<EnumInfo>,
    /// Raw definitions kept for field resolution.
    struct_defs: RawStructDefs,
    enum_defs: RawEnumDefs,
    struct_spans: Vec<Span>,
}

impl TypeTables {
    pub fn struct_ty(&self, id: StructId) -> &StructInfo {
        &self.structs[id.0 as usize]
    }

    pub fn enum_ty(&self, id: EnumId) -> &EnumInfo {
        &self.enums[id.0 as usize]
    }

    /// Resolves a type expression to an IR type.
    // CEP:WHAT: Handles the resolve step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn resolve(&self, ty: &TypeExpr, diags: &mut Vec<Diag>) -> Option<Ty> {
        match ty {
            TypeExpr::Name { name, span } => {
                let resolved = match name.as_str() {
                    "i8" => Some(Ty::I8),
                    "i16" => Some(Ty::I16),
                    "i32" => Some(Ty::I32),
                    "i64" => Some(Ty::I64),
                    "u8" => Some(Ty::U8),
                    "u16" => Some(Ty::U16),
                    "u32" => Some(Ty::U32),
                    "u64" => Some(Ty::U64),
                    "f32" => Some(Ty::F32),
                    "f64" => Some(Ty::F64),
                    "bool" => Some(Ty::Bool),
                    "char" => Some(Ty::Char),
                    "()" => Some(Ty::Unit),
                    "str" => Some(Ty::Str),
                    _ => None,
                };
                if let Some(ty) = resolved {
                    return Some(ty);
                }
                if let Some(id) = self.struct_ids.get(name) {
                    return Some(Ty::Struct(*id));
                }
                if let Some(id) = self.enum_ids.get(name) {
                    return Some(Ty::Enum(*id));
                }
                diags.push(Diag::error(
                    ERR_UNKNOWN_TYPE,
                    *span,
                    format!("unknown type `{}`", name),
                ));
                None
            }
            TypeExpr::Ref { mutable, inner, .. } => {
                let inner_ty = self.resolve(inner, diags)?;
                Some(Ty::Ref(Box::new(inner_ty), *mutable))
            }
            TypeExpr::Array { elem, size, .. } => {
                let elem_ty = self.resolve(elem, diags)?;
                let size = (*size).min(u32::MAX as u64);
                Some(Ty::Array(Box::new(elem_ty), size))
            }
        }
    }

    fn struct_size(&self, id: StructId) -> u32 {
        self.structs[id.0 as usize].size
    }

    fn struct_align(&self, id: StructId) -> u32 {
        self.structs[id.0 as usize].align
    }

    fn enum_size(&self, id: EnumId) -> u32 {
        self.enums[id.0 as usize].size
    }

    fn enum_align(&self, id: EnumId) -> u32 {
        self.enums[id.0 as usize].align
    }

    pub(crate) fn type_size(&self, ty: &Ty) -> u32 {
        match ty {
            Ty::Struct(id) => self.struct_size(*id),
            Ty::Enum(id) => self.enum_size(*id),
            Ty::Array(elem, size) => self.type_size(elem).saturating_mul(*size as u32),
            other => other.scalar_size().unwrap_or(8),
        }
    }

    pub(crate) fn type_align(&self, ty: &Ty) -> u32 {
        match ty {
            Ty::Struct(id) => self.struct_align(*id),
            Ty::Enum(id) => self.enum_align(*id),
            Ty::Array(elem, _) => self.type_align(elem),
            other => scalar_align(other),
        }
    }

    /// Lays out one struct by id, memoized; detects value cycles.
    // CEP:WHAT: Handles the compute struct step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn compute_struct(&mut self, id: StructId, active: &mut BTreeSet<u32>, diags: &mut Vec<Diag>) {
        if self.structs[id.0 as usize].size != 0 || self.structs[id.0 as usize].align != 0 {
            return;
        }
        if !active.insert(id.0) {
            let span = self.struct_spans[id.0 as usize];
            diags.push(Diag::error(
                ERR_RECURSIVE_TYPE,
                span,
                "struct type recursively contains itself by value".to_string(),
            ));
            active.remove(&id.0);
            return;
        }
        let (name, raw_fields) = {
            let (name, fields) = &self.struct_defs[id.0 as usize];
            (name.clone(), fields.clone())
        };
        let mut laid: Vec<(String, Ty, u32)> = Vec::with_capacity(raw_fields.len());
        let mut offset: u32 = 0;
        let mut align: u32 = 1;
        for (field_name, field_ty_expr) in &raw_fields {
            let field_ty = match self.resolve(field_ty_expr, diags) {
                Some(ty) => ty,
                None => Ty::Unit,
            };
            match &field_ty {
                Ty::Struct(inner) => self.compute_struct(*inner, active, diags),
                Ty::Enum(inner) => self.compute_enum(*inner, active, diags),
                Ty::Array(elem, _) => match elem.as_ref() {
                    Ty::Struct(inner) => self.compute_struct(*inner, active, diags),
                    Ty::Enum(inner) => self.compute_enum(*inner, active, diags),
                    _ => {}
                },
                _ => {}
            }
            let field_align = self.type_align(&field_ty);
            offset = align_up(offset, field_align);
            let field_offset = offset;
            offset = offset.saturating_add(self.type_size(&field_ty));
            align = align.max(field_align);
            laid.push((field_name.clone(), field_ty, field_offset));
        }
        let size = align_up(offset, align);
        self.structs[id.0 as usize] = StructInfo {
            name,
            fields: laid,
            size,
            align,
        };
        active.remove(&id.0);
    }

    // CEP:WHAT: Handles the compute enum step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    fn compute_enum(&mut self, id: EnumId, active: &mut BTreeSet<u32>, diags: &mut Vec<Diag>) {
        if self.enums[id.0 as usize].size != 0 {
            return;
        }
        if !active.insert(id.0) {
            diags.push(Diag::error(
                ERR_RECURSIVE_TYPE,
                Span::new(0, 0),
                "enum type recursively contains itself by value".to_string(),
            ));
            active.remove(&id.0);
            return;
        }
        let (name, raw_variants) = {
            let (name, variants) = &self.enum_defs[id.0 as usize];
            (name.clone(), variants.clone())
        };
        let mut laid: Vec<(String, Option<Ty>)> = Vec::with_capacity(raw_variants.len());
        let mut align: u32 = 4;
        let mut max_payload: u32 = 0;
        for (variant_name, payload_expr) in &raw_variants {
            let payload = payload_expr
                .as_ref()
                .and_then(|expr| self.resolve(expr, diags));
            if let Some(ty) = &payload {
                match ty {
                    Ty::Struct(inner) => self.compute_struct(*inner, active, diags),
                    Ty::Enum(inner) => self.compute_enum(*inner, active, diags),
                    _ => {}
                }
                max_payload = max_payload.max(self.type_size(ty));
                align = align.max(self.type_align(ty));
            }
            laid.push((variant_name.clone(), payload));
        }
        let payload_offset = align_up(4, align);
        let size = align_up(payload_offset.saturating_add(max_payload), align);
        self.enums[id.0 as usize] = EnumInfo {
            name,
            variants: laid,
            size,
            align,
            payload_offset,
        };
        active.remove(&id.0);
    }
}

/// Builds tables from module items: registers names, resolves fields, computes layout.
// CEP:WHAT: Handles the build tables step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn build_tables(module: &Module, diags: &mut Vec<Diag>) -> TypeTables {
    let mut tables = TypeTables {
        struct_ids: BTreeMap::new(),
        enum_ids: BTreeMap::new(),
        structs: Vec::new(),
        enums: Vec::new(),
        struct_defs: Vec::new(),
        enum_defs: Vec::new(),
        struct_spans: Vec::new(),
    };
    for item in &module.items {
        match item {
            Item::Struct(def) => register_struct(&mut tables, def, diags),
            Item::Enum(def) => register_enum(&mut tables, def, diags),
            _ => {}
        }
    }
    let mut active: BTreeSet<u32> = BTreeSet::new();
    for id in 0..tables.structs.len() as u32 {
        tables.compute_struct(StructId(id), &mut active, diags);
    }
    for id in 0..tables.enums.len() as u32 {
        tables.compute_enum(EnumId(id), &mut active, diags);
    }
    tables
}

// CEP:WHAT: Handles the register struct step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn register_struct(tables: &mut TypeTables, def: &StructDef, diags: &mut Vec<Diag>) {
    if tables.struct_ids.contains_key(&def.name.name)
        || tables.enum_ids.contains_key(&def.name.name)
    {
        diags.push(Diag::error(
            ERR_DUPLICATE_DEF,
            def.name.span,
            format!("type `{}` is defined more than once", def.name.name),
        ));
        return;
    }
    let id = StructId(tables.structs.len() as u32);
    tables.struct_ids.insert(def.name.name.clone(), id);
    tables.struct_defs.push((
        def.name.name.clone(),
        def.fields
            .iter()
            .map(|(name, ty)| (name.name.clone(), ty.clone()))
            .collect(),
    ));
    tables.struct_spans.push(def.name.span);
    tables.structs.push(StructInfo {
        name: def.name.name.clone(),
        fields: Vec::new(),
        size: 0,
        align: 0,
    });
}

// CEP:WHAT: Handles the register enum step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn register_enum(tables: &mut TypeTables, def: &EnumDef, diags: &mut Vec<Diag>) {
    if tables.struct_ids.contains_key(&def.name.name)
        || tables.enum_ids.contains_key(&def.name.name)
    {
        diags.push(Diag::error(
            ERR_DUPLICATE_DEF,
            def.name.span,
            format!("type `{}` is defined more than once", def.name.name),
        ));
        return;
    }
    let id = EnumId(tables.enums.len() as u32);
    tables.enum_ids.insert(def.name.name.clone(), id);
    tables.enum_defs.push((
        def.name.name.clone(),
        def.variants
            .iter()
            .map(|(name, payload)| (name.name.clone(), payload.clone()))
            .collect(),
    ));
    tables.enums.push(EnumInfo {
        name: def.name.name.clone(),
        variants: Vec::new(),
        size: 0,
        align: 0,
        payload_offset: 0,
    });
}

/// Copy classification per design Part 1 ("every value has exactly one owner;
/// moving transfers ownership; copying requires explicit `Clone`").
///
/// Scalars (`i8..u64`, `f32/f64`, `bool`, `char`), the unit type, references
/// (`&T`, `&mut T`, `&str`) are `Copy`: reading them never severs the source's
/// ownership. Structs and enums are **not** `Copy`: every use that takes the
/// value by value is a move, enforced by the move checker (PLK-010). Arrays are
/// `Copy` exactly when their element type is, mirroring the value semantics of
/// repeated element copy-out. Explicit `Clone` support arrives with traits
/// (ticket PLK-004); until then a non-`Copy` value can only be re-obtained by
/// reassignment, not by duplication.
pub fn is_copy(ty: &Ty) -> bool {
    match ty {
        Ty::Struct(_) | Ty::Enum(_) => false,
        Ty::Array(elem, _) => is_copy(elem),
        _ => true,
    }
}
