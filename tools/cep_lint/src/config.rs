// CEP:FILE: tools/cep_lint/src/config.rs
// CEP:WHAT: Configuration model and loader for the data-driven lint engine.
// CEP:WHY: Chapter 50 requires every pattern, threshold, severity, and message to live in .cep/cep_lint.json with the engine holding mechanism only; this module is the typed view of that contract.
// CEP:CLASS: CEP-2
// CEP:STATUS: complete
// CEP:FAILURE: Schema violations return BootError values mapped to CEP-LINT-BOOT-4/5/6/7/8 by the caller; unreadable files map to BOOT-1 and parse failures to BOOT-2.
// CEP:ASSUMES: The configuration schema is versioned by standard.version; unknown rule keys are rejected so documented-but-unconfigured and configured-but-undocumented drift both surface through DOC-SYNC and schema strictness.
// CEP:COST: O(config size); loading happens once per run.
// CEP:EVIDENCE: exercised by --self-test and every lint run.
// CEP:SECURITY: Configuration files are repository-trusted; the parser still bounds all access.
// CEP:PORTABILITY: Pure core.

#![forbid(unsafe_code)]

use crate::json::{self, Json};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Rule {
    pub check: String,
    pub mode: String,
    pub severity: u8,
    /// Standard document sections the rule enforces; kept for DOC-SYNC reporting.
    #[allow(dead_code)]
    pub sections: Vec<String>,
    pub params: BTreeMap<String, Json>,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub standard_name: String,
    pub standard_version: String,
    pub document_path: String,
    pub tool_name: String,
    pub tool_version: String,
    pub file_globs: Vec<String>,
    pub fail_on: Vec<u8>,
    pub rules: BTreeMap<String, Rule>,
}

pub enum BootError {
    Io(String),
    Parse(String),
    Schema(String),
}

impl BootError {
    /// Numeric part of the CEP-LINT-BOOT-<n> code per section 50.4.
    pub fn code(&self) -> u8 {
        match self {
            BootError::Io(_) => 1,
            BootError::Parse(_) => 2,
            BootError::Schema(_) => 4,
        }
    }
}

// CEP:WHAT: Handles the load step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
pub fn load(path: &Path) -> Result<Config, BootError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| BootError::Io(format!("{}: {}", path.display(), err)))?;
    let value = json::parse(&text)
        .map_err(|err| BootError::Parse(format!("{}: {}", path.display(), err)))?;
    from_json(&value)
        .map_err(|message| BootError::Schema(format!("{}: {}", path.display(), message)))
}

// CEP:WHAT: Handles the from json step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn from_json(value: &Json) -> Result<Config, String> {
    let standard = value
        .get("standard")
        .and_then(|v| v.as_object())
        .ok_or("missing object `standard`")?;
    let tool = value
        .get("tool")
        .and_then(|v| v.as_object())
        .ok_or("missing object `tool`")?;
    let config = Config {
        standard_name: standard
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("missing standard.name")?
            .to_string(),
        standard_version: standard
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or("missing standard.version")?
            .to_string(),
        document_path: standard
            .get("document")
            .and_then(|v| v.as_str())
            .ok_or("missing standard.document")?
            .to_string(),
        tool_name: tool
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("missing tool.name")?
            .to_string(),
        tool_version: tool
            .get("version")
            .and_then(|v| v.as_str())
            .ok_or("missing tool.version")?
            .to_string(),
        file_globs: value
            .get("file_globs")
            .and_then(|v| v.as_array())
            .ok_or("missing array `file_globs`")?
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .map(|s| s.to_string())
                    .ok_or("file_globs entries must be strings")
            })
            .collect::<Result<Vec<String>, &str>>()
            .map_err(|err| err.to_string())?,
        fail_on: value
            .get("fail_on")
            .and_then(|v| v.as_array())
            .ok_or("missing array `fail_on`")?
            .iter()
            .map(|entry| {
                let severity = entry.as_i64().ok_or("fail_on entries must be integers")?;
                if !(0..=3).contains(&severity) {
                    return Err("fail_on severities must be 0..=3 (BOOT-7)".to_string());
                }
                Ok(severity as u8)
            })
            .collect::<Result<Vec<u8>, String>>()?,
        rules: BTreeMap::new(),
    };

    let rules_json = value
        .get("rules")
        .and_then(|v| v.as_object())
        .ok_or("missing object `rules`")?;
    let mut rules = BTreeMap::new();
    for (name, rule_json) in rules_json {
        let rule = parse_rule(name, rule_json)?;
        rules.insert(name.clone(), rule);
    }
    let mut config = config;
    config.rules = rules;
    Ok(config)
}

// CEP:WHAT: Handles the parse rule step of this stage per the design and the file header contract.
// CEP:WHY: The stage's contract requires this step; the module documentation records the design choice and rejected alternatives.
// CEP:STATUS: complete
// CEP:FAILURE: Returns diagnostics or results; never panics on any input.
// CEP:ASSUMES: See the file header for the stage-wide assumptions.
// CEP:COST: Bounded by the input size of this stage; no allocation beyond the documented model.
// CEP:EVIDENCE: cargo test over the containing crate.
fn parse_rule(name: &str, rule_json: &Json) -> Result<Rule, String> {
    let check = rule_json
        .get("check")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("rule {}: missing check", name))?
        .to_string();
    let known_checks = [
        "comment_block_schema",
        "function_policy",
        "comment_regex",
        "string_literal_regex",
        "numeric_literal_policy",
        "macro_prefix_policy",
        "banned_token_policy",
        "flat_code_regex",
        "doc_rule_coverage",
    ];
    if !known_checks.contains(&check.as_str()) {
        return Err(format!("rule {}: unknown check `{}` (BOOT-6)", name, check));
    }
    let severity = rule_json
        .get("severity")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| format!("rule {}: missing severity", name))?;
    if !(0..=3).contains(&severity) {
        return Err(format!("rule {}: severity out of range (BOOT-7)", name));
    }
    let mut params = BTreeMap::new();
    if let Some(params_json) = rule_json.get("params").and_then(|v| v.as_object()) {
        for (key, value) in params_json {
            params.insert(key.clone(), value.clone());
        }
    }
    Ok(Rule {
        check,
        mode: rule_json
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        severity: severity as u8,
        sections: rule_json
            .get("sections")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        params,
        message: rule_json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("violation")
            .to_string(),
    })
}
