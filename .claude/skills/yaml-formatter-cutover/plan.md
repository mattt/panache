# In-tree YAML formatter cutover plan

Staged plan for retiring `yaml_parser` and `pretty_yaml` in favor of an
in-tree YAML formatter driven by the in-tree YAML CST. Sibling document
to `SKILL.md`. Annotate the **What landed** block as work progresses,
matching the `scanner-rewrite.md` precedent in `yaml-shadow-expand/`.

## Status

- **Phase 1 (shadow formatter):** in progress. 1.1 module skeleton
  landed (byte-passthrough stub); 1.2 STYLE.md relocated; 1.3
  cross-validation harness landed with starter corpus; rule
  implementations (1.4+) outstanding.
- **Phase 2 (joint cutover):** not started, blocked on Phase 1.
- **Phase 3 (hashpipe extension):** not started, blocked on Phase 2.

## What landed since drafting

_(Update as phases complete. Earliest entries on top.)_

- **Phase 1.3 — cross-validation harness.** Added
  `crates/panache-formatter/tests/yaml_cross_validation.rs`, which
  discovers every `*.yaml` under
  `crates/panache-formatter/tests/fixtures/yaml_corpus/` and, per
  case, asserts (a) `format_yaml(input) == pretty_yaml::format_text(input)`
  with options bridged the same way `yaml_engine.rs` bridges them
  (`print_width` ← `line_width`, `prose_wrap` ← `wrap`, everything
  else at pretty_yaml defaults) and (b) `format_yaml(format_yaml(x)) ==
  format_yaml(x)`. Failures accumulate into one panic so a batch of
  red cases is visible at once. Seeded the corpus with 8 trivially-
  canonical inputs (simple/two-key/nested mappings, top-level + nested
  sequences, leading comment, short flow sequence, doc-start marker)
  that round-trip through pretty_yaml's defaults — chosen so the
  Phase 1.1 byte-passthrough stub passes parity and idempotency
  today. The plan's Phase 1.3 "corpus seeding" intent (real
  frontmatter extracts, hand-picked stressors for flow overflow /
  anchors / multi-line scalars) deferred to land alongside the rule
  implementations that make each case pass — adding them now would
  just enumerate divergences, which is exactly what the
  yaml-formatter rule forbids. yaml.rs module doc-comment updated to
  reflect the 1.3 status. No live-pipeline changes.
- **Phase 1.2 — STYLE.md relocation.** Moved the 13-rule style spec
  out of this plan into
  `crates/panache-formatter/src/formatter/yaml/STYLE.md` (canonical
  home). Added a pointer from `docs/guide/formatting.qmd` in the
  YAML frontmatter section so user-facing docs reach the spec.
  Updated the `crates/panache-formatter/src/formatter/yaml.rs`
  module doc-comment to cite `STYLE.md` instead of the now-relocated
  plan-side spec. No behavior change; the formatter module is still
  the Phase 1.1 byte-passthrough stub. Plan retains rollout context
  and references STYLE.md from the spec section below.
- **Phase 1.1 — module skeleton.** Added
  `crates/panache-formatter/src/formatter/yaml.rs` (parent) and the
  six submodule files (`options.rs`, `document.rs`, `block_map.rs`,
  `block_sequence.rs`, `flow.rs`, `scalar.rs`) under
  `crates/panache-formatter/src/formatter/yaml/`. Public entry
  `format_yaml(text, &YamlFormatOptions) -> String` calls
  `panache_parser::parser::yaml::parse_yaml_tree`, walks the CST, and
  emits tokens verbatim (byte-lossless stub — applies no style rules
  yet). Module wired into `formatter.rs` as `pub mod yaml;` behind an
  `#[allow(dead_code)]` shadow marker; not reachable from the live
  pipeline. Compiles clean; clippy clean; two unit-test smokes pass.
  Plan amended to spell out the no-`mod.rs` layout rule, matching the
  project convention from AGENTS.md.

## Context

The in-tree streaming YAML parser is event-parity complete against
yaml-test-suite (`crates/panache-parser/tests/yaml/triage.json`:
308 passes_now, 94 error_contract_ok, both `fails_needs_*` buckets
empty). It has a lossless CST and a delegated scalar-cooking module.

It has no formatter consumer. The live pipeline still uses the legacy
`yaml_parser` crate via `crates/panache-parser/src/syntax/yaml.rs` for
the CST, and `pretty_yaml::format_text` via
`crates/panache-formatter/src/yaml_engine.rs` for output. The in-tree
parser is therefore unproven on the dimensions a formatter would
exercise — CST shape (trivia attachment, comment placement, indent
grouping) rather than event stream.

A pure parser cutover would swap internals with no user-visible
payoff; its parity bar is too weak to catch shape gaps. A formatter
gives the cutover a downstream consumer and a real parity bar.

## Goals

- One pipeline end-to-end: in-tree parser → in-tree formatter.
- `yaml_parser` and `pretty_yaml` both retired in the cutover commit.
- **Rule-based deterministic style** — output follows the style spec
  below, not a tool's whims. pretty_yaml is used as a cross-validation
  reference because it implements the same rules; it is not the
  source of truth.
- Strong idempotency invariant: `format(format(x)) == format(x)`
  asserted in the corpus harness, not as a separate test.
- Plain metadata first; hashpipe inherits via existing
  `normalize_hashpipe_input` once Phase 2 lands.

## Non-goals

- Replacing yaml-test-suite event parity. That bar stays.
- Tracking pretty_yaml's choices when they conflict with the style
  spec. If pretty_yaml ever drifts from the spec on an edge case, we
  follow the spec and either fix pretty_yaml upstream or work around
  in the corpus harness.
- Wiring the in-tree formatter into the live path before Phase 2.

## Style spec

The canonical 13-rule style spec lives in
[`crates/panache-formatter/src/formatter/yaml/STYLE.md`](../../../crates/panache-formatter/src/formatter/yaml/STYLE.md).
That file is the source of truth for what the in-tree formatter
emits; this plan tracks rollout, not the spec itself.

The spec is deterministic (same input → same output) and was
cross-validated against pretty_yaml 0.6.0 and Prettier 3.6.2 on a
15-case battery of representative frontmatter — both agree on rules
1–12; rule 6's bracket placement is the one point where they differ,
and the rule pins pretty_yaml's choice. Rule 13 (trailing document
newline) is not yet cross-validated; that gets done as part of the
Phase 1.3 corpus harness.

Adding a 14th rule is a deliberate act and follows the process
documented in [`yaml-formatter`](../../rules/yaml-formatter.md): a new
rule in STYLE.md with a one-line rationale and a fixture under
`crates/panache-formatter/tests/fixtures/yaml_corpus/`, plus an
explicit decision when it conflicts with pretty_yaml's behavior.

## Phase 1 — Shadow in-tree formatter (plain metadata)

Build `crates/panache-formatter/src/formatter/yaml/` consuming the
in-tree parser CST. Not wired to the live pipeline.

### 1.1 — Module skeleton

Follow the project's modern-Rust layout convention: a parent `yaml.rs`
file declares the submodules; per-feature code sits in sibling files
under `yaml/`. **No `mod.rs`** anywhere in the tree (see AGENTS.md).

- `crates/panache-formatter/src/formatter/yaml.rs` — parent module.
  Public entry: `format_yaml(text: &str, opts: &YamlFormatOptions) -> String`.
  Declares the submodules below.
- `crates/panache-formatter/src/formatter/yaml/` — submodule files:
  - `document.rs` — top-level document orchestration.
  - `block_map.rs`, `block_sequence.rs`, `flow.rs`, `scalar.rs` —
    per-CST-node rendering.
  - `options.rs` — `YamlFormatOptions` (line-width, wrap mode, quote
    style preference, …).
- Wire into the formatter crate by adding `pub mod yaml;` to
  `crates/panache-formatter/src/formatter.rs`.
- Initial entry calls into in-tree parser via
  `panache_parser::parser::yaml::parse_yaml_tree(text)`, walks the
  returned CST, emits text.

### 1.2 — Move style spec into the module

Landed: the 13-rule spec lives in
`crates/panache-formatter/src/formatter/yaml/STYLE.md`, with a
pointer from `docs/guide/formatting.qmd` (YAML frontmatter section).
This plan no longer carries the spec; it tracks rollout only.

If Phase 1 development discovers a 14th rule (an edge case neither
the spec nor pretty_yaml currently covers), add it to STYLE.md with
a fixture and a one-line rationale. New rules need cross-validation
against pretty_yaml before landing — if they conflict, decide
explicitly which is right and document the decision.

### 1.3 — Cross-validation harness

New test file
`crates/panache-formatter/tests/yaml_cross_validation.rs`. For each
case in the corpus:

1. Read `input.yaml`.
2. `let in_tree = panache_formatter::formatter::yaml::format_yaml(input, &opts);`
3. `let pretty = pretty_yaml::format_text(input, &opts)?;`
4. Assert `in_tree == pretty` (rule 6's bracket placement matches
   pretty_yaml, so this should hold across the corpus).
5. Assert `format_yaml(in_tree, ...) == in_tree` (idempotency).
6. If `in_tree != pretty`: it's a bug in (a) the in-tree formatter,
   (b) the in-tree parser CST shape, or (c) pretty_yaml. Diagnose
   and fix — do NOT add the case to a divergence list. The corpus
   is calibration data for the spec, not a divergence registry.

Corpus seeding:
- Pull real frontmatter from existing
  `tests/fixtures/cases/*/input.{md,qmd,Rmd}` (extract the YAML
  region).
- Add `crates/panache-formatter/tests/fixtures/yaml_corpus/` with
  hand-picked cases that stress comments, multi-line scalars,
  anchors, tags, and flow overflow (rule 6).
- Optionally cycle in a slice of the yaml-test-suite plain cases that
  pretty_yaml handles cleanly.

### 1.4 — CST shape gaps surfaced by the harness

Expected outcome of Phase 1 is a list of parser-side fixes driven by
formatter symptoms. Track each fix as a separate parser commit (per
[`formatter`](../../rules/formatter.md) rule on idempotency
root-causing).

### Exit criteria for Phase 1

- Every corpus case satisfies `in_tree == pretty` and idempotency.
- STYLE.md is the canonical spec; this plan no longer carries it.
- Any parser CST shape gaps surfaced by the harness are fixed in
  `panache-parser` (separate commits).

## Phase 2 — Joint cutover

When Phase 1 exits, swap parser and formatter in one commit.

### 2.1 — Parser side

- Update `crates/panache-parser/src/syntax/yaml.rs` to call the
  in-tree parser (`parse_yaml_report`) and surface its CST shape into
  the host CST.
- Audit downstream consumers of the YAML CST shape: linter rules,
  LSP, anything that walks
  `SyntaxKind::YAML_*` nodes. The in-tree parser's `YAML_*` kinds
  must already be the host CST's kinds for this to be a no-op (verify
  before cutover).

### 2.2 — Formatter side

- Replace `crates/panache-formatter/src/yaml_engine.rs::format_text`
  call with `formatter::yaml::format_yaml`.
- Remove the `pretty_yaml` dependency from
  `crates/panache-formatter/Cargo.toml`.
- Remove the `yaml_parser` dependency from `Cargo.toml` (root).

### 2.3 — Golden case regen

Expect host-level golden cases under `tests/fixtures/cases/*/` to
shift on YAML-affecting cases. Each delta must:
- Match the style spec (and pretty_yaml's output, by construction), or
- Be a fix for a known bug captured by a `tests/yaml_corpus/` case, or
- Be challenged before accepting.

### Exit criteria for Phase 2

- `yaml_parser` and `pretty_yaml` removed from `Cargo.lock`.
- All host golden cases green; deltas annotated.
- `cargo test` workspace green.
- Triage of parser-side regressions (if any) — should be zero per the
  shape audit, but verify.

## Phase 3 — Hashpipe extension

Same parser + formatter, exercised through the existing hashpipe
normalization path.

### 3.1 — Wire-up

- `crates/panache-formatter/src/formatter/hashpipe.rs` already calls
  the YAML engine for option bodies. Re-point it to
  `formatter::yaml::format_yaml` with hashpipe normalization.
- Confirm `normalize_hashpipe_input` behaviour matches what the
  formatter expects (it strips `#|`; the formatter re-prefixes).

### 3.2 — Hashpipe-specific fixtures

Add cases under
`crates/panache-formatter/tests/fixtures/yaml_corpus/hashpipe/` for:
- Continuation lines (`#| key: value\n#|   continued`).
- Blank-line semantics inside `#|`.
- Anchors / tags in chunk options.
- The existing `issue_*_hashpipe_*` host fixtures should drop their
  pretty_yaml-specific quirks at this point — re-check each.

### Exit criteria for Phase 3

- Hashpipe and plain metadata share one formatter path governed by
  the same style spec.
- All host hashpipe golden cases green; pretty_yaml-specific
  workarounds in `crates/panache-formatter/src/formatter/hashpipe.rs`
  removed.

## Open questions

- **YamlFormatOptions surface.** Mirror pretty_yaml's option surface
  in the in-tree formatter, or design our own from scratch? Mirroring
  eases the cutover; designing fresh avoids inheriting quirks. Note:
  the spec is fixed; options control orthogonal knobs like
  `line-width` and `prose-wrap`, not style choices.
- **Salsa integration.** Does the formatter need its own salsa input,
  or piggyback on the parser's `YamlInput` from
  `crates/panache-parser/src/parser/yaml/model.rs`?
- **Style-as-CST-kind promotion.** Deferred in `scanner-rewrite.md`,
  but the formatter may force it (rule 4 requires distinguishing
  `|` / `>` / `'…'` / `"…"` styles per-scalar). Decide before Phase
  1.1 lands whether to do this preemptively or reactively.
- **Lossless parser preservation of trailing newline.** Rule 13 pins
  the formatter output (always one `\n` at EOF) but the in-tree
  parser must round-trip the trailing newline byte-for-byte (zero,
  one, or many) so the formatter has a deterministic input signal.
  Verify in Phase 1.1.
