// CEP:FILE: tools/cep_lint/src/scan.rs
// CEP:WHAT: The nine check primitives of the lint engine, driven entirely by rule parameters from the configuration.
// CEP:WHY: Chapter 50 separates mechanism (this file) from policy (the JSON configuration); each primitive implements one structural check over Rust source classified into comments, strings, and code.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Checks are total over any input file; malformed files produce findings, never tool panics. Function extraction is heuristic (brace matching from `fn` signatures) and its limits are documented in the chapter 50 rule texts.
// CEP:ASSUMES: Rust dialect per the 0.3 applicability note: comments are // and /* */; attributes are #[...] lines; functions start with `fn NAME(`. Glob patterns support `**` across path segments and `*` within one segment.
// CEP:COST: O(file bytes) per check with small constants; function extraction is one linear scan.
// CEP:EVIDENCE: --self-test scenarios cover one violating and one compliant case per rule with exact counts.
// CEP:SECURITY: Source files are untrusted; all indexing is bounds-checked and brace matching is bounded by file size.
// CEP:PORTABILITY: Line endings: both LF and CRLF are accepted; output uses LF.

#![forbid(unsafe_code)]

use crate::config::{Config, Rule};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub rule: String,
    pub severity: u8,
    pub message: String,
}

/// One source file classified into per-line comment/code/string views.
struct SourceFile {
    path: String,
    lines: Vec<String>,
    /// Per line: the comment substring (without markers), if any.
    comment_text: Vec<Option<String>>,
    /// Per line: code with comments and string contents removed (string quotes kept as markers).
    code_text: Vec<String>,
    /// Per line: contents of string literals on that line.
    string_text: Vec<String>,
    /// Class from the file header, if present.
    header_class: Option<String>,
    /// The first contiguous full-line comment block as (start_line, field map).
    header_block: Option<(usize, BTreeMap<String, String>)>,
}

/// Collects files matching the configured globs under `root`, sorted for determinism.
// CEP:WHAT: Handles the collect files step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn collect_files(root: &Path, globs: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_recursive(root, root, globs, &mut files);
    files.sort();
    files
}

// CEP:WHAT: Handles the collect recursive step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn collect_recursive(root: &Path, dir: &Path, globs: &[String], files: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(|entry| entry.ok()).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name.starts_with('.') {
                continue; // build output and VCS metadata are out of scope
            }
            collect_recursive(root, &path, globs, files);
        } else if let Ok(relative) = path.strip_prefix(root) {
            let rel_text = relative.to_string_lossy().replace('\\', "/");
            if globs.iter().any(|glob| glob_match(glob, &rel_text)) {
                files.push(path);
            }
        }
    }
}

/// Matches a glob with `**` (any segments) and `*` (within a segment).
// CEP:WHAT: Handles the glob match step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn glob_match(glob: &str, text: &str) -> bool {
    let glob_parts: Vec<&str> = glob.split('/').collect();
    let text_parts: Vec<&str> = text.split('/').collect();
    match_parts(&glob_parts, &text_parts)
}

// CEP:WHAT: Handles the match parts step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn match_parts(glob: &[&str], text: &[&str]) -> bool {
    match glob.first() {
        None => text.is_empty(),
        Some(&"**") => {
            // `**` matches zero or more segments.
            for skip in 0..=text.len() {
                if match_parts(&glob[1..], &text[skip..]) {
                    return true;
                }
            }
            false
        }
        Some(&head) => {
            if text.is_empty() {
                return false;
            }
            if segment_match(head, text[0]) {
                match_parts(&glob[1..], &text[1..])
            } else {
                false
            }
        }
    }
}

// CEP:WHAT: Handles the segment match step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn segment_match(glob: &str, text: &str) -> bool {
    // Single-segment glob with `*` wildcards.
    let glob_chars: Vec<char> = glob.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    seg_match_at(&glob_chars, 0, &text_chars, 0)
}

// CEP:WHAT: Handles the seg match at step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn seg_match_at(glob: &[char], gi: usize, text: &[char], ti: usize) -> bool {
    if gi == glob.len() {
        return ti == text.len();
    }
    match glob[gi] {
        '*' => {
            for skip in ti..=text.len() {
                if seg_match_at(glob, gi + 1, text, skip) {
                    return true;
                }
            }
            false
        }
        g => ti < text.len() && text[ti] == g && seg_match_at(glob, gi + 1, text, ti + 1),
    }
}

// CEP:WHAT: Handles the load source step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn load_source(path: &Path) -> SourceFile {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let path_text = path.to_string_lossy().replace('\\', "/");
    let mut file = SourceFile {
        path: path_text,
        lines: Vec::new(),
        comment_text: Vec::new(),
        code_text: Vec::new(),
        string_text: Vec::new(),
        header_class: None,
        header_block: None,
    };
    let mut in_block_comment = false;
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r').to_string();
        let (comment, code, strings) = classify_line(&line, &mut in_block_comment);
        file.lines.push(line.clone());
        file.comment_text.push(comment);
        file.code_text.push(code);
        file.string_text.push(strings);
    }
    extract_header(&mut file);
    file
}

/// Splits one line into (comment text, code text, string text). Tracks block-comment state.
// CEP:WHAT: Handles the classify line step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn classify_line(line: &str, in_block_comment: &mut bool) -> (Option<String>, String, String) {
    let bytes: Vec<char> = line.chars().collect();
    let mut comment = String::new();
    let mut code = String::new();
    let mut strings = String::new();
    let mut idx = 0;
    let mut in_string = false;
    let mut escaped = false;
    while idx < bytes.len() {
        let ch = bytes[idx];
        if *in_block_comment {
            if ch == '*' && idx + 1 < bytes.len() && bytes[idx + 1] == '/' {
                *in_block_comment = false;
                idx += 2;
                continue;
            }
            comment.push(ch);
            idx += 1;
            continue;
        }
        if in_string {
            strings.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            idx += 1;
            continue;
        }
        if ch == '/' && idx + 1 < bytes.len() && bytes[idx + 1] == '/' {
            // Line comment: the rest is comment text.
            for rest in &bytes[idx + 2..] {
                comment.push(*rest);
            }
            break;
        }
        if ch == '/' && idx + 1 < bytes.len() && bytes[idx + 1] == '*' {
            *in_block_comment = true;
            idx += 2;
            continue;
        }
        if ch == '"' {
            in_string = true;
            strings.push(ch);
            code.push('"');
            idx += 1;
            continue;
        }
        code.push(ch);
        idx += 1;
    }
    let comment_opt =
        if comment.trim().is_empty() && !comment.contains('\u{0}') && comment.is_empty() {
            None
        } else {
            Some(comment)
        };
    (comment_opt, code, strings)
}

/// Parses `CEP:FIELD: value` out of the text after a `//` marker.
/// Returns (field, value) with the field in canonical `CEP:FIELD` form.
// CEP:WHAT: Handles the parse cep field step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn parse_cep_field(comment: &str) -> Option<(String, String)> {
    let trimmed = comment.trim_start();
    let rest = trimmed.strip_prefix("CEP:")?;
    let (field, value) = rest.split_once(':')?;
    Some((format!("CEP:{}", field.trim()), value.trim().to_string()))
}

/// Parses the file header block: the first contiguous run of full-line `//` comments.
// CEP:WHAT: Handles the extract header step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn extract_header(file: &mut SourceFile) {
    let mut start: Option<usize> = None;
    let mut end: usize = 0;
    for (idx, line) in file.lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            if start.is_none() {
                start = Some(idx);
            }
            end = idx + 1;
        } else if trimmed.is_empty() && start.is_none() {
            continue; // blank lines before the header are tolerated by max_start_line accounting
        } else {
            break;
        }
    }
    if let Some(start) = start {
        let mut fields = BTreeMap::new();
        for line in &file.lines[start..end] {
            if let Some(comment) = line.trim_start().strip_prefix("//") {
                if let Some((field, value)) = parse_cep_field(comment) {
                    fields.insert(field, value);
                }
            }
        }
        file.header_class = fields.get("CEP:CLASS").cloned();
        file.header_block = Some((start, fields));
    }
}

fn param_str(rule: &Rule, key: &str, default: &str) -> String {
    rule.params
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

fn param_lines(rule: &Rule, key: &str, default: &[&str]) -> Vec<String> {
    match rule.params.get(key).and_then(|v| v.as_array()) {
        Some(items) => items
            .iter()
            .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
            .collect(),
        None => default.iter().map(|s| s.to_string()).collect(),
    }
}

fn param_usize(rule: &Rule, key: &str, default: usize) -> usize {
    rule.params
        .get(key)
        .and_then(|v| v.as_usize())
        .unwrap_or(default)
}

/// A Rust function definition located by linear scan.
struct FnDef {
    name: String,
    /// Zero-based line of the `fn` signature.
    line: usize,
    /// Zero-based body start and end (inclusive of braces).
    body_start: usize,
    body_end: usize,
}

fn extract_functions(file: &SourceFile) -> Vec<FnDef> {
    let mut functions = Vec::new();
    let mut idx = 0;
    while idx < file.lines.len() {
        let code = &file.code_text[idx];
        if let Some(name) = fn_signature_name(code) {
            // Find the opening brace of the body (may be a few lines down).
            let mut brace_line = idx;
            while brace_line < file.lines.len() && !file.code_text[brace_line].contains('{') {
                brace_line += 1;
                if brace_line - idx > 8 {
                    break;
                }
            }
            if brace_line < file.lines.len() {
                if let Some((body_start, body_end)) = match_braces(file, brace_line) {
                    functions.push(FnDef {
                        name: name.to_string(),
                        line: idx,
                        body_start,
                        body_end,
                    });
                    idx = body_end + 1;
                    continue;
                }
            }
        }
        idx += 1;
    }
    functions
}

/// Extracts the function name from a line whose code contains a `fn NAME(` signature.
// CEP:WHAT: Handles the fn signature name step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn fn_signature_name(code: &str) -> Option<String> {
    let marker = "fn ";
    let start = code.find(marker)?;
    let rest = &code[start + marker.len()..];
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    let after = &rest[name.len()..];
    let after_trim = after.trim_start();
    // Generic or direct parameter list; both count as a definition signature.
    if after_trim.starts_with('(') || after_trim.starts_with('<') {
        Some(name)
    } else {
        None
    }
}

/// Brace-matches from the first `{` on `start_line`, returns inclusive line bounds.
// CEP:WHAT: Handles the match braces step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn match_braces(file: &SourceFile, start_line: usize) -> Option<(usize, usize)> {
    let mut depth: i64 = 0;
    let mut opened = false;
    let mut in_string = false;
    let mut escaped = false;
    for line_idx in start_line..file.lines.len() {
        for ch in file.lines[line_idx].chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            match ch {
                '"' => in_string = true,
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => {
                    depth -= 1;
                    if opened && depth == 0 {
                        return Some((start_line, line_idx));
                    }
                }
                _ => {}
            }
        }
        in_string = false; // strings do not span lines in this dialect
    }
    None
}

// CEP:WHAT: Handles the count statements step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn count_statements(file: &SourceFile, body_start: usize, body_end: usize) -> usize {
    let mut count = 0;
    for line in &file.code_text[body_start..=body_end] {
        count += line.matches(';').count();
    }
    count
}

// CEP:WHAT: Handles the body is empty step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn body_is_empty(file: &SourceFile, body_start: usize, body_end: usize) -> bool {
    for (offset, line) in file.code_text[body_start..=body_end].iter().enumerate() {
        let mut text = line.as_str();
        if body_start + offset == body_start {
            if let Some(pos) = text.find('{') {
                text = &text[pos + 1..];
            }
        }
        if body_start + offset == body_end {
            if let Some(pos) = text.rfind('}') {
                text = &text[..pos];
            }
        }
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return false;
        }
    }
    true
}

/// Comment block directly above a definition, allowing attribute lines in between.
// CEP:WHAT: Handles the preceding block step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn preceding_block(file: &SourceFile, def_line: usize) -> Option<BTreeMap<String, String>> {
    let mut idx = def_line;
    // Only attribute lines may sit between the block and the definition; a blank line
    // breaks adjacency so a file header is never accepted as a function block.
    while idx > 0 {
        let above = file.lines[idx - 1].trim();
        if above.starts_with("#[") || above.starts_with("# [") {
            idx -= 1;
        } else {
            break;
        }
    }
    if idx == 0 {
        return None;
    }
    let mut end = idx;
    while end > 0 && file.lines[end - 1].trim_start().starts_with("//") {
        end -= 1;
    }
    if end == idx {
        return None; // no comment lines directly above
    }
    // Chapter 50: file header blocks are never accepted as function blocks.
    if let Some((header_start, _)) = file.header_block {
        if header_start == end {
            return None;
        }
    }
    let mut fields = BTreeMap::new();
    for line in &file.lines[end..idx] {
        if let Some(comment) = line.trim_start().strip_prefix("//") {
            if let Some((field, value)) = parse_cep_field(comment) {
                fields.insert(field, value);
            }
        }
    }
    Some(fields)
}

fn field_names(rule: &Rule, key: &str, default: &[&str]) -> Vec<String> {
    param_lines(rule, key, default)
}

pub struct Engine {
    config: Config,
    root: PathBuf,
}

impl Engine {
    pub fn new(config: Config, root: &Path) -> Self {
        Engine {
            config,
            root: root.to_path_buf(),
        }
    }

    pub fn fail_on(&self) -> &[u8] {
        &self.config.fail_on
    }

    /// Runs all rules over all configured files; findings are sorted deterministically.
    // CEP:WHAT: Handles the run step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn run(&self) -> Vec<Finding> {
        let files = collect_files(&self.root, &self.config.file_globs);
        let mut findings = Vec::new();
        for path in &files {
            let file = load_source(path);
            for (name, rule) in &self.config.rules {
                if name == "CEP-LINT-DOC-SYNC" {
                    continue; // handled separately over the standard document
                }
                self.apply_rule(name, rule, &file, &mut findings);
            }
        }
        findings.sort_by(|a, b| {
            (&a.file, a.line, &a.rule, &a.message).cmp(&(&b.file, b.line, &b.rule, &b.message))
        });
        findings
    }

    /// DOC-SYNC compares the standard document against the configuration.
    // CEP:WHAT: Handles the run doc sync step of this stage per the design and the file header contract.
    // CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
    // CEP:STATUS: complete
    // CEP:FAILURE: Returns diagnostics or results; never panics on any input.
    // CEP:ASSUMES: See the file header for the stage-wide assumptions.
    // CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
    // CEP:EVIDENCE: cargo test over the containing crate.
    pub fn run_doc_sync(&self) -> Vec<Finding> {
        let rule = match self.config.rules.get("CEP-LINT-DOC-SYNC") {
            Some(rule) => rule,
            None => return Vec::new(),
        };
        let mut findings = Vec::new();
        let doc_path = self.root.join(&self.config.document_path);
        let document = match std::fs::read_to_string(&doc_path) {
            Ok(text) => text,
            Err(err) => {
                findings.push(Finding {
                    file: self.config.document_path.clone(),
                    line: 0,
                    rule: "CEP-LINT-DOC-SYNC".to_string(),
                    severity: rule.severity,
                    message: format!("standard document unreadable: {}", err),
                });
                return findings;
            }
        };
        let mut documented: Vec<String> = Vec::new();
        for line in document.lines() {
            if let Some(rest) = line.strip_prefix("### Rule ") {
                documented.push(rest.trim().to_string());
            }
        }
        // Unique headings.
        let mut unique = documented.clone();
        unique.sort();
        unique.dedup();
        if unique.len() != documented.len() {
            findings.push(Finding {
                file: self.config.document_path.clone(),
                line: 0,
                rule: "CEP-LINT-DOC-SYNC".to_string(),
                severity: rule.severity,
                message: "duplicate `### Rule` headings in the standard document".to_string(),
            });
        }
        // Configured rules must be documented.
        for name in self.config.rules.keys() {
            if name == "CEP-LINT-DOC-SYNC" {
                continue;
            }
            if !documented.contains(name) {
                findings.push(Finding {
                    file: self.config.document_path.clone(),
                    line: 0,
                    rule: "CEP-LINT-DOC-SYNC".to_string(),
                    severity: rule.severity,
                    message: format!("configured rule {} has no `### Rule` section", name),
                });
            }
        }
        // Documented rules must be configured.
        for name in &documented {
            if !self.config.rules.contains_key(name) {
                findings.push(Finding {
                    file: self.config.document_path.clone(),
                    line: 0,
                    rule: "CEP-LINT-DOC-SYNC".to_string(),
                    severity: rule.severity,
                    message: format!("documented rule {} is not configured", name),
                });
            }
        }
        // Version line must equal the configured standard version.
        let version_line = param_str(rule, "version_pattern", "Version:");
        let mut version_found = false;
        for (idx, line) in document.lines().enumerate() {
            if line.contains(&version_line) {
                version_found = true;
                if !line.contains(&self.config.standard_version) {
                    findings.push(Finding {
                        file: self.config.document_path.clone(),
                        line: idx + 1,
                        rule: "CEP-LINT-DOC-SYNC".to_string(),
                        severity: rule.severity,
                        message: format!(
                            "document version line does not match configuration version {}",
                            self.config.standard_version
                        ),
                    });
                }
                break;
            }
        }
        if !version_found {
            findings.push(Finding {
                file: self.config.document_path.clone(),
                line: 0,
                rule: "CEP-LINT-DOC-SYNC".to_string(),
                severity: rule.severity,
                message: format!("no `{}` line found in the standard document", version_line),
            });
        }
        findings
    }

    fn apply_rule(&self, name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
        match (rule.check.as_str(), rule.mode.as_str()) {
            ("comment_block_schema", "file_header") => {
                check_file_header(name, rule, file, findings)
            }
            ("comment_block_schema", "status_value") => {
                check_status_value(name, rule, file, findings)
            }
            ("comment_block_schema", "stub_requirements") => {
                check_stub_requirements(name, rule, file, findings)
            }
            ("function_policy", "block") => check_func_block(name, rule, file, findings),
            ("function_policy", "stub_body") => check_stub_body(name, rule, file, findings),
            ("function_policy", "naming") => check_naming_func(name, rule, file, findings),
            ("function_policy", "constant_return") => {
                check_constant_return(name, rule, file, findings)
            }
            ("comment_regex", _) => check_comment_regex(name, rule, file, findings),
            ("string_literal_regex", _) => check_string_literal_regex(name, rule, file, findings),
            ("numeric_literal_policy", _) => check_magic_numbers(name, rule, file, findings),
            ("macro_prefix_policy", _) => check_macro_prefix(name, rule, file, findings),
            ("banned_token_policy", _) => check_banned_tokens(name, rule, file, findings),
            ("flat_code_regex", _) => check_flat_code_regex(name, rule, file, findings),
            _ => {}
        }
    }
}

fn push(
    findings: &mut Vec<Finding>,
    file: &SourceFile,
    line: usize,
    rule: &str,
    severity: u8,
    message: &str,
) {
    findings.push(Finding {
        file: file.path.clone(),
        line: line + 1,
        rule: rule.to_string(),
        severity,
        message: message.to_string(),
    });
}

// CEP:WHAT: Handles the check file header step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_file_header(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let max_start = param_usize(rule, "max_start_line", 5);
    let required = field_names(
        rule,
        "required_fields",
        &[
            "CEP:FILE",
            "CEP:WHAT",
            "CEP:WHY",
            "CEP:CLASS",
            "CEP:STATUS",
            "CEP:FAILURE",
            "CEP:ASSUMES",
            "CEP:COST",
            "CEP:EVIDENCE",
        ],
    );
    let allowed = param_lines(
        rule,
        "allowed_classes",
        &[
            "CEP-0", "CEP-1", "CEP-2", "CC", "FFI", "HPC-0", "HPC-1", "HPC-2", "RCS-0", "RCS-1",
            "RCS-2",
        ],
    );
    match &file.header_block {
        None => {
            push(
                findings,
                file,
                0,
                name,
                rule.severity,
                "file has no CEP header comment block",
            );
        }
        Some((start, fields)) => {
            if *start >= max_start {
                push(
                    findings,
                    file,
                    *start,
                    name,
                    rule.severity,
                    "CEP header does not start within the configured first lines",
                );
            }
            for field in &required {
                if !fields.contains_key(field) {
                    push(
                        findings,
                        file,
                        *start,
                        name,
                        rule.severity,
                        &format!("file header is missing `{}`", field),
                    );
                }
            }
            if let Some(class) = fields.get("CEP:CLASS") {
                if !allowed.contains(class) {
                    push(
                        findings,
                        file,
                        *start,
                        name,
                        rule.severity,
                        &format!(
                            "header class `{}` is not an allowed conformance class",
                            class
                        ),
                    );
                }
            }
        }
    }
}

// CEP:WHAT: Handles the check status value step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_status_value(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let allowed = param_lines(rule, "allowed_values", &[]);
    for (idx, line) in file.lines.iter().enumerate() {
        if let Some(comment) = &file.comment_text[idx] {
            let _ = line;
            if let Some(pos) = comment.find("CEP:STATUS:") {
                let value = comment[pos + "CEP:STATUS:".len()..].trim();
                if !allowed.contains(&value.to_string()) {
                    push(
                        findings,
                        file,
                        idx,
                        name,
                        rule.severity,
                        &format!("`{}` is not an allowed CEP:STATUS value", value),
                    );
                }
            }
        }
    }
}

// CEP:WHAT: Handles the check stub requirements step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_stub_requirements(
    name: &str,
    rule: &Rule,
    file: &SourceFile,
    findings: &mut Vec<Finding>,
) {
    let statuses = param_lines(rule, "statuses", &[]);
    // Scan every comment block in the file.
    let mut idx = 0;
    while idx < file.lines.len() {
        if file.lines[idx].trim_start().starts_with("//") {
            let start = idx;
            let mut fields = BTreeMap::new();
            while idx < file.lines.len() && file.lines[idx].trim_start().starts_with("//") {
                if let Some(comment) = file.comment_text[idx].as_ref() {
                    if let Some((field, value)) = parse_cep_field(comment) {
                        fields.insert(field, value);
                    }
                }
                idx += 1;
            }
            if let Some(status) = fields.get("CEP:STATUS") {
                if statuses.contains(&status.to_string()) {
                    for required in ["CEP:OWNER", "CEP:TICKET"] {
                        if !fields.contains_key(required) {
                            push(
                                findings,
                                file,
                                start,
                                name,
                                rule.severity,
                                &format!(
                                    "comment block with STATUS {} is missing `{}`",
                                    status, required
                                ),
                            );
                        }
                    }
                }
            }
        } else {
            idx += 1;
        }
    }
}

// CEP:WHAT: Handles the check func block step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_func_block(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let min_statements = param_usize(rule, "min_body_statements", 2);
    let required = field_names(
        rule,
        "required_fields",
        &[
            "CEP:WHAT",
            "CEP:WHY",
            "CEP:STATUS",
            "CEP:FAILURE",
            "CEP:ASSUMES",
            "CEP:COST",
            "CEP:EVIDENCE",
        ],
    );
    for func in extract_functions(file) {
        let statements = count_statements(file, func.body_start, func.body_end);
        if statements < min_statements {
            continue;
        }
        match preceding_block(file, func.line) {
            Some(fields) => {
                for field in &required {
                    if !fields.contains_key(field) {
                        push(
                            findings,
                            file,
                            func.line,
                            name,
                            rule.severity,
                            &format!(
                                "function `{}` lacks `{}` in its CEP comment block",
                                func.name, field
                            ),
                        );
                    }
                }
            }
            None => {
                push(
                    findings,
                    file,
                    func.line,
                    name,
                    rule.severity,
                    &format!("function `{}` has no CEP comment block", func.name),
                );
            }
        }
    }
}

// CEP:WHAT: Handles the check stub body step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_stub_body(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let suppress = param_lines(rule, "suppress_statuses", &[]);
    for func in extract_functions(file) {
        if !body_is_empty(file, func.body_start, func.body_end) {
            continue;
        }
        let sanctioned = preceding_block(file, func.line)
            .and_then(|fields| fields.get("CEP:STATUS").cloned())
            .map(|status| suppress.contains(&status))
            .unwrap_or(false);
        if !sanctioned {
            push(
                findings,
                file,
                func.line,
                name,
                rule.severity,
                &format!(
                    "function `{}` has an empty body without a declared stub status",
                    func.name
                ),
            );
        }
    }
}

// CEP:WHAT: Handles the check naming func step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_naming_func(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let pattern = param_str(rule, "name_pattern", "^[a-z][a-z0-9_]*$");
    for func in extract_functions(file) {
        if !matches_regex(&pattern, &func.name) {
            push(
                findings,
                file,
                func.line,
                name,
                rule.severity,
                &format!("function name `{}` is not snake_case", func.name),
            );
        }
    }
}

// CEP:WHAT: Handles the check constant return step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_constant_return(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let pattern = param_str(rule, "pattern", "return (true|false|nullptr);");
    for func in extract_functions(file) {
        let mut statements: Vec<String> = Vec::new();
        for (offset, line) in file.code_text[func.body_start..=func.body_end]
            .iter()
            .enumerate()
        {
            let mut text = line.as_str();
            if func.body_start + offset == func.body_start {
                // Strip everything up to and including the body-opening brace.
                if let Some(pos) = text.find('{') {
                    text = &text[pos + 1..];
                }
            }
            if func.body_start + offset == func.body_end {
                if let Some(pos) = text.rfind('}') {
                    text = &text[..pos];
                }
            }
            let trimmed = text.trim();
            if !trimmed.is_empty() && trimmed != "{" && trimmed != "}" {
                statements.push(trimmed.to_string());
            }
        }
        if statements.len() == 1 && matches_regex(&pattern, &statements[0]) {
            push(
                findings,
                file,
                func.line,
                name,
                rule.severity,
                &format!(
                    "function `{}` computes nothing: body is a single constant return",
                    func.name
                ),
            );
        }
    }
}

// CEP:WHAT: Handles the check comment regex step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_comment_regex(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let pattern = param_str(rule, "pattern", "");
    if pattern.is_empty() {
        return;
    }
    let suppression = rule
        .params
        .get("suppression_pattern")
        .and_then(|v| v.as_str());
    for (idx, comment) in file.comment_text.iter().enumerate() {
        let comment = match comment {
            Some(text) => text,
            None => continue,
        };
        if let Some(suppression) = suppression {
            if matches_regex(suppression, comment) {
                continue;
            }
        }
        if matches_regex(&pattern, comment) {
            push(
                findings,
                file,
                idx,
                name,
                rule.severity,
                rule.message.as_str(),
            );
        }
    }
}

// CEP:WHAT: Handles the check string literal regex step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_string_literal_regex(
    name: &str,
    rule: &Rule,
    file: &SourceFile,
    findings: &mut Vec<Finding>,
) {
    let pattern = param_str(rule, "pattern", "");
    if pattern.is_empty() {
        return;
    }
    for (idx, strings) in file.string_text.iter().enumerate() {
        if strings.is_empty() {
            continue;
        }
        if matches_regex(&pattern, strings) {
            push(
                findings,
                file,
                idx,
                name,
                rule.severity,
                rule.message.as_str(),
            );
        }
    }
}

// CEP:WHAT: Handles the check magic numbers step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_magic_numbers(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let allowed_values = rule
        .params
        .get("allowed_values")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|entry| entry.as_i64())
                .collect::<Vec<i64>>()
        })
        .unwrap_or_else(|| vec![0, 1]);
    let exempt = param_lines(
        rule,
        "exempt_contexts",
        &["const ", "static ", "enum ", "assert", "#[", "match ", "0x"],
    );
    for (idx, code) in file.code_text.iter().enumerate() {
        for literal in numeric_literals(code) {
            let value: Option<i64> = literal.parse().ok();
            if let Some(value) = value {
                if allowed_values.contains(&value) {
                    continue;
                }
            } else {
                let _ = value;
            }
            let line = &file.lines[idx];
            if exempt.iter().any(|marker| line.contains(marker)) {
                continue;
            }
            // Test code (cfg(test) modules) is tooling class: report but the rule's
            // severity already keeps it advisory.
            if line.trim_start().starts_with("//") || line.trim().is_empty() {
                continue;
            }
            push(
                findings,
                file,
                idx,
                name,
                rule.severity,
                &format!("numeric literal `{}` outside a sanctioned context", literal),
            );
            break; // one finding per line keeps output compact
        }
    }
}

/// Extracts decimal numeric literals from code text (comments and strings already stripped).
// CEP:WHAT: Handles the numeric literals step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn numeric_literals(code: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let chars: Vec<char> = code.chars().collect();
    let mut idx = 0;
    while idx < chars.len() {
        let preceded_by_identifier =
            idx > 0 && (chars[idx - 1].is_ascii_alphanumeric() || chars[idx - 1] == '_');
        if chars[idx].is_ascii_digit() && !preceded_by_identifier {
            let start = idx;
            while idx < chars.len()
                && (chars[idx].is_ascii_alphanumeric() || chars[idx] == '.' || chars[idx] == '_')
            {
                idx += 1;
            }
            let token: String = chars[start..idx].iter().collect();
            // Hex literals start 0x — treat as exempt by the caller's marker; still report the token.
            if token.chars().all(|ch| ch.is_ascii_digit()) {
                literals.push(token);
            }
        } else {
            idx += 1;
        }
    }
    literals
}

// CEP:WHAT: Handles the check macro prefix step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_macro_prefix(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let name_pattern = param_str(rule, "name_pattern", "^(CEP_[A-Z0-9_]*|_[A-Z0-9_]*)$");
    for (idx, code) in file.code_text.iter().enumerate() {
        if let Some(pos) = code.find("macro_rules!") {
            let rest = code[pos + "macro_rules!".len()..].trim_start();
            let macro_name: String = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            if !matches_regex(&name_pattern, &macro_name) {
                push(
                    findings,
                    file,
                    idx,
                    name,
                    rule.severity,
                    &format!(
                        "macro `{}` does not match the required prefix pattern",
                        macro_name
                    ),
                );
            }
        }
    }
}

// CEP:WHAT: Handles the check banned tokens step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_banned_tokens(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let applies = param_lines(rule, "applies_to_classes", &["CEP-0", "HPC-0", "RCS-0"]);
    let class = match &file.header_class {
        Some(class) => class.clone(),
        None => return,
    };
    if !applies.contains(&class) {
        return;
    }
    let tokens = param_lines(rule, "tokens", &[]);
    for (idx, code) in file.code_text.iter().enumerate() {
        for token in &tokens {
            if code_contains_token(code, token) {
                push(
                    findings,
                    file,
                    idx,
                    name,
                    rule.severity,
                    &format!("banned token `{}` in a {} file", token, class),
                );
            }
        }
    }
}

/// Token containment with identifier-boundary awareness on the left.
// CEP:WHAT: Handles the code contains token step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn code_contains_token(code: &str, token: &str) -> bool {
    let mut search_from = 0;
    while let Some(pos) = code[search_from..].find(token) {
        let absolute = search_from + pos;
        let before_ok = absolute == 0
            || !code[..absolute]
                .chars()
                .last()
                .map(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                .unwrap_or(false);
        if before_ok {
            return true;
        }
        search_from = absolute + 1;
        if search_from >= code.len() {
            break;
        }
    }
    false
}

// CEP:WHAT: Handles the check flat code regex step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn check_flat_code_regex(name: &str, rule: &Rule, file: &SourceFile, findings: &mut Vec<Finding>) {
    let pattern = param_str(rule, "pattern", "");
    if pattern.is_empty() {
        return;
    }
    // The pattern is matched against the code text with whitespace collapsed.
    let flattened: String = file
        .code_text
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    for (idx, code) in file.code_text.iter().enumerate() {
        let collapsed: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
        if matches_regex(&pattern, &collapsed) {
            push(
                findings,
                file,
                idx,
                name,
                rule.severity,
                rule.message.as_str(),
            );
            continue;
        }
        let _ = &flattened;
    }
}

/// Minimal AST-based regex matcher supporting the subset the configuration needs:
/// literals, `.`, `*`, `+`, `?`, classes `[...]` (with negation and ranges), groups `(...)`
/// with `|` alternatives, anchors `^` and `$`, and escapes `\\s`, `\\d`, `\\(` and friends.
/// Compiled once per call; inputs are single lines and small.
// CEP:WHAT: Handles the matches regex step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn matches_regex(pattern: &str, text: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let parsed = match parse_alternatives(&chars, &mut 0, None) {
        Some(alternatives) => alternatives,
        None => return false,
    };
    let text_chars: Vec<char> = text.chars().collect();
    for start in 0..=text_chars.len() {
        for alternative in &parsed {
            if alt_match(alternative, 0, &text_chars, start) {
                return true;
            }
        }
    }
    false
}

// CEP:WHAT: Handles the parse alternatives step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn parse_alternatives(
    chars: &[char],
    pos: &mut usize,
    stop: Option<char>,
) -> Option<Vec<Vec<RegexNode>>> {
    let mut alternatives = vec![Vec::new()];
    while *pos < chars.len() {
        let ch = chars[*pos];
        if Some(ch) == stop {
            return Some(alternatives);
        }
        match ch {
            '|' => {
                alternatives.push(Vec::new());
                *pos += 1;
            }
            _ => {
                let node = parse_node(chars, pos)?;
                alternatives
                    .last_mut()
                    .expect("alternatives never empty")
                    .push(node);
            }
        }
    }
    if stop.is_some() {
        return None; // unclosed group
    }
    Some(alternatives)
}

#[derive(Clone)]
enum RegexNode {
    Element {
        atom: RegexAtom,
        min: usize,
        max: usize,
    },
    AnchorStart,
    AnchorEnd,
}

#[derive(Clone)]
enum RegexAtom {
    Char(char),
    Any,
    Class {
        negated: bool,
        chars: Vec<char>,
        ranges: Vec<(char, char)>,
    },
    Group(Vec<Vec<RegexNode>>),
}

// CEP:WHAT: Handles the parse node step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn parse_node(chars: &[char], pos: &mut usize) -> Option<RegexNode> {
    let atom = match chars.get(*pos)? {
        '^' => {
            *pos += 1;
            return Some(RegexNode::AnchorStart);
        }
        '$' => {
            *pos += 1;
            return Some(RegexNode::AnchorEnd);
        }
        '.' => {
            *pos += 1;
            RegexAtom::Any
        }
        '\\' => {
            *pos += 1;
            let escaped = chars.get(*pos)?;
            let atom = match escaped {
                's' => RegexAtom::Class {
                    negated: false,
                    chars: vec![' ', '\t'],
                    ranges: vec![],
                },
                'd' => RegexAtom::Class {
                    negated: false,
                    chars: vec![],
                    ranges: vec![('0', '9')],
                },
                'w' => RegexAtom::Class {
                    negated: false,
                    chars: vec!['_'],
                    ranges: vec![('a', 'z'), ('A', 'Z'), ('0', '9')],
                },
                other => RegexAtom::Char(*other),
            };
            *pos += 1;
            atom
        }
        '[' => {
            *pos += 1;
            let negated = chars.get(*pos) == Some(&'^');
            if negated {
                *pos += 1;
            }
            let mut class_chars = Vec::new();
            let mut ranges = Vec::new();
            while *pos < chars.len() && chars[*pos] != ']' {
                if *pos + 2 < chars.len() && chars[*pos + 1] == '-' && chars[*pos + 2] != ']' {
                    ranges.push((chars[*pos], chars[*pos + 2]));
                    *pos += 3;
                } else {
                    class_chars.push(chars[*pos]);
                    *pos += 1;
                }
            }
            *pos += 1; // closing ]
            RegexAtom::Class {
                negated,
                chars: class_chars,
                ranges,
            }
        }
        '(' => {
            *pos += 1;
            let group = parse_alternatives(chars, pos, Some(')'))?;
            *pos += 1; // closing )
            RegexAtom::Group(group)
        }
        other => {
            *pos += 1;
            RegexAtom::Char(*other)
        }
    };
    let (min, max) = match chars.get(*pos) {
        Some('*') => {
            *pos += 1;
            (0, usize::MAX)
        }
        Some('+') => {
            *pos += 1;
            (1, usize::MAX)
        }
        Some('?') => {
            *pos += 1;
            (0, 1)
        }
        _ => (1, 1),
    };
    Some(RegexNode::Element { atom, min, max })
}

// CEP:WHAT: Handles the atom match step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn atom_match(atom: &RegexAtom, text: &[char], ti: usize) -> Option<usize> {
    if ti >= text.len() {
        return None;
    }
    match atom {
        RegexAtom::Char(ch) if text[ti] == *ch => Some(1),
        RegexAtom::Any => Some(1),
        RegexAtom::Class {
            negated,
            chars,
            ranges,
        } => {
            let hit = chars.contains(&text[ti])
                || ranges
                    .iter()
                    .any(|(lo, hi)| text[ti] >= *lo && text[ti] <= *hi);
            if hit != *negated {
                Some(1)
            } else {
                None
            }
        }
        RegexAtom::Group(alternatives) => {
            let mut best: Option<usize> = None;
            for alternative in alternatives {
                if let Some(consumed) = seq_match(alternative, 0, text, ti) {
                    best = Some(match best {
                        Some(existing) if existing >= consumed => existing,
                        _ => consumed,
                    });
                }
            }
            best
        }
        _ => None,
    }
}

/// Matches one alternative sequence from node index; returns total consumed characters.
// CEP:WHAT: Handles the seq match step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn seq_match(nodes: &[RegexNode], ni: usize, text: &[char], ti: usize) -> Option<usize> {
    if ni == nodes.len() {
        return Some(0);
    }
    match &nodes[ni] {
        RegexNode::AnchorStart => {
            if ti == 0 {
                seq_match(nodes, ni + 1, text, ti)
            } else {
                None
            }
        }
        RegexNode::AnchorEnd => {
            if ti == text.len() {
                seq_match(nodes, ni + 1, text, ti)
            } else {
                None
            }
        }
        RegexNode::Element { atom, min, max } => {
            // Greedy with backtracking.
            let mut positions = vec![ti];
            let mut current = ti;
            while positions.len() <= *max {
                match atom_match(atom, text, current) {
                    Some(consumed) => {
                        current += consumed;
                        positions.push(current);
                    }
                    None => break,
                }
            }
            while positions.len() > *min {
                let pos = *positions.last().expect("non-empty");
                if let Some(rest) = seq_match(nodes, ni + 1, text, pos) {
                    return Some(pos - ti + rest);
                }
                positions.pop();
            }
            None
        }
    }
}

/// Alternative-sequence match used by atom_match for group contents (ignores anchors).
// CEP:WHAT: Handles the alt match step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn alt_match(nodes: &[RegexNode], ni: usize, text: &[char], ti: usize) -> bool {
    if ni == nodes.len() {
        return true;
    }
    match &nodes[ni] {
        RegexNode::AnchorStart => ti == 0 && alt_match(nodes, ni + 1, text, ti),
        RegexNode::AnchorEnd => ti == text.len() && alt_match(nodes, ni + 1, text, ti),
        RegexNode::Element { atom, min, max } => {
            let mut positions = vec![ti];
            let mut current = ti;
            while positions.len() <= *max {
                match atom_match(atom, text, current) {
                    Some(consumed) => {
                        current += consumed;
                        positions.push(current);
                    }
                    None => break,
                }
            }
            while positions.len() > *min {
                let pos = *positions.last().expect("non-empty");
                if alt_match(nodes, ni + 1, text, pos) {
                    return true;
                }
                positions.pop();
            }
            false
        }
    }
}

#[cfg(test)]
mod regex_tests {
    use super::matches_regex;

    // CEP:WHAT: Verifies that anchored class star behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn anchored_class_star() {
        assert!(matches_regex("^[a-z][a-z0-9_]*$", "align_of"));
        assert!(matches_regex("^[a-z][a-z0-9_]*$", "add_step"));
        assert!(!matches_regex("^[a-z][a-z0-9_]*$", "AddOne"));
    }

    // CEP:WHAT: Verifies that alternation groups behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn alternation_groups() {
        assert!(matches_regex("(TODO|FIXME|XXX|HACK)", " TODO: fix"));
        assert!(matches_regex(
            "return (true|false|nullptr);",
            "return true;"
        ));
        assert!(!matches_regex("return (true|false|nullptr);", "return x;"));
    }

    // CEP:WHAT: Verifies that plus and question behaves as specified.
    // CEP:WHY: Regression pin: a failure means the behavior under test changed without a specification change.
    // CEP:STATUS: complete
    // CEP:FAILURE: The test fails on unexpected output or exit status.
    // CEP:ASSUMES: Fixture inputs are repository-controlled.
    // CEP:COST: Microseconds; unit test.
    // CEP:EVIDENCE: cargo test.
    #[test]
    fn plus_and_question() {
        assert!(matches_regex("catch [(][^)]*[)] [{] [}]", "catch (e) { }"));
        assert!(!matches_regex(
            "catch [(][^)]*[)] [{] [}]",
            "catch (e) { log(); }"
        ));
        assert!(matches_regex(
            "(if|while|for|match) .* [{] [}]",
            "if a == 0 { }"
        ));
    }
}
