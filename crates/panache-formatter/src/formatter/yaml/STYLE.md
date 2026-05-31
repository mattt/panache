# YAML formatter style spec

Canonical reference for the deterministic style rules that govern the in-tree
YAML formatter under `crates/panache-formatter/src/formatter/yaml/`.

These rules are deterministic (same input → same output) and small enough to fit
in one table. They were cross-validated against pretty_yaml 0.6.0 and Prettier
3.6.2 on a 15-case battery of representative frontmatter --- both agree on rules
1--12; rule 6's bracket placement is the one point where they differ, and the
rule pins pretty_yaml's choice.

pretty_yaml is the cross-validation reference because it implements the same
rules. It is not the source of truth: this document is. If the formatter
diverges from pretty_yaml on a case, one of the two is wrong relative to this
spec --- fix it; do not enumerate it. See `.claude/rules/yaml-formatter.md` for
the load-bearing invariants.

## Rules

1. **Indent.** 2 spaces, fully canonicalized regardless of input shape.
2. **Sequence items** indented +2 from the parent key (`categories:\n  - foo`,
   never `- foo` at parent column).
3. **Quote style preference:** plain → double-quoted → single-quoted only when
   content contains characters that would need backslash-escaping in
   double-quoted form (e.g. `'C:\Users\test'`).
4. **Block scalar style** (literal `|` vs folded `>`): preserved from input.
   They carry different YAML semantics and are not interchangeable.
5. **Flow spacing:** `{ key: value }` with spaces inside braces; `[a, b, c]`
   with a space after each comma.
6. **Flow wrap on line-width overflow:** each item on its own line, trailing
   comma, **opening bracket stays on the key line**
   (`keywords: [\n  first,\n  ...\n]`). This is the one point of disagreement
   between pretty_yaml and Prettier --- we follow pretty_yaml.
7. **Blank lines:** runs of multiple blank lines collapse to one max.
8. **Inline comments:** exactly one space before `#`.
9. **Comment positions** (above key, inline, between keys): preserved. Comments
   are user-authored content.
10. **Trailing whitespace** on every line: stripped.
11. **Empty scalars:** `key:` stays `key:`, never canonicalized to `key: null`
    or `key: ""`.
12. **Key order:** preserved. Frontmatter is content the user wrote; reordering
    would surprise.
13. **Trailing document newline:** always exactly one `\n` at EOF. Missing
    trailing newline → add one; multiple trailing newlines → collapse to one.
    Not cross-validated against pretty_yaml or Prettier yet; needed in Phase 1.3
    corpus harness.

## Notes

Rules 4, 9, and 12 are "preserve" rules: they don't add a new behavior, they
explicitly decline to canonicalize a semantically-meaningful user choice.
They're still deterministic.

Rule 3 is the only spec rule with semantic-content awareness. The
escape-required test is decidable from the scalar's bytes alone (no context
dependence), so it remains rule-based.

## Plain-scalar wrapping (config, not spec)

Plain-scalar wrapping is a config option, not a spec rule. It is controlled by
Panache's `wrap` setting, which `yaml_engine.rs` maps onto pretty_yaml's
`ProseWrap`:

- `wrap: preserve` → `ProseWrap::Preserve` --- nothing wraps.
- `wrap: reflow` (default) / `sentence` / `semantic` → `ProseWrap::Always` ---
  plain scalars wrap with +2 indent continuation lines; quoted (`"…"`, `'…'`)
  and block (`>`, `|`) styles never wrap regardless of mode.

The in-tree formatter inherits this mapping at cutover. The spec-adjacent
invariant worth pinning: **only plain scalars are ever wrapped; quoted and block
styles are preserved verbatim regardless of wrap mode**. Wrapping a quoted
scalar would change escape behavior (double-quoted) or require backslash
handling not present in single-quoted; wrapping a block scalar would change `>`
folding or `|` literal semantics.

Edge case worth knowing about: a plain scalar containing `key: value`-shaped
text (colon followed by space, mid-content) is already ambiguous to strict YAML
parsers; wrapping it surfaces the breakage. The in-tree parser will likely
reject this input outright, making the wrap question moot. If we ever silently
accept it, the formatter must avoid wrapping at that boundary.

## Adding a new rule

Adding a 14th rule is a deliberate act. If Phase 1 development surfaces an edge
case neither the spec nor pretty_yaml currently covers, the resolution is a new
rule here (with a one-line rationale and a fixture under
`crates/panache-formatter/tests/fixtures/yaml_corpus/`) --- not a special-case
branch in the formatter.

New rules need cross-validation against pretty_yaml before landing. If they
conflict, decide explicitly which is right and document the decision. See
`.claude/rules/yaml-formatter.md` and
`.claude/skills/yaml-formatter-cutover/plan.md` for the process context.
