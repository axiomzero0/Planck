# cep_lint

## Abstract

cep_lint is the repository's mechanical enforcement tool per standard chapter 50. Its inputs are a JSON configuration and Rust source files; its outputs are sorted findings on stdout and an exit code. The engine holds mechanism only: every pattern, threshold, severity, and message lives in `.cep/cep_lint.json`.

## Formal Definitions

```rust
pub struct Config { pub standard_version: String, pub file_globs: Vec<String>, pub fail_on: Vec<u8>, pub rules: BTreeMap<String, Rule> }
pub struct Rule { pub check: String, pub mode: String, pub severity: u8, pub params: BTreeMap<String, Json>, pub message: String }
pub struct Finding { pub file: String, pub line: usize, pub rule: String, pub severity: u8, pub message: String }
```

Severity mapping (chapter 34.2): 0 exterminate immediately, 1 exterminate unless waiver, 2 reject and repair, 3 warn. `fail_on: [0, 1]` blocks merges.

## Algorithm

1. Load the configuration; schema violations report `CEP-LINT-BOOT-4..8`, unreadable files `BOOT-1`, parse failures `BOOT-2` with line and column.
2. Collect files matching the globs (sorted traversal; build output and dot-directories excluded).
3. Per file: classify each line into comment, code, and string views; extract the file header and function definitions (brace matching from `fn` signatures).
4. Apply the seventeen rules through nine check primitives: comment_block_schema (header, status, stub modes), function_policy (block, stub_body, naming, constant_return), comment_regex, string_literal_regex, numeric_literal_policy, macro_prefix_policy, banned_token_policy, flat_code_regex.
5. DOC-SYNC compares the configuration against chapter 50: every configured rule needs a `### Rule` heading, every documented rule needs configuration, headings must be unique, and the document's `Version:` line must equal `standard.version`.
6. Findings sort by file, line, rule, message; exit 1 when any finding's severity is in `fail_on`.

## Invariants

1. Deterministic: directory traversal, finding order, and output depend only on the input tree and configuration.
2. The tool never panics on any input file or configuration.
3. Self-test: one violating and one compliant scenario per configured rule with exact expected counts (manifest at `tools/cep_lint/tests/manifest.json`); all thirty must pass.
4. Self-lint: the tool's own source and the whole workspace lint clean at severity 0 and 1.

## Integration Points

- Depends on: nothing outside the workspace; zero external crates.
- Depended on by: CI (the lint gate), developers (pre-commit checks).

Signatures: `cep_lint [--config <path>] [--self-test] [--version] [paths...]`; exit codes 0/1/2/3 per chapter 50.1.

## Known Limitations

- Lexical and structural only: allocation behavior, cost claims, and macro-generated control flow still require review and evidence (chapter 50.5).
- Function extraction is a heuristic; macros generating functions are invisible to it.
- Constant-name style is deliberately unchecked until the standard resolves the 6.2/33.15 conflict (chapter 50.5).
