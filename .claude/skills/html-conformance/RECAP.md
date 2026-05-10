# HTML conformance — running session recap

Rolling, terse handoff between sessions of the `html-conformance`
skill. Read at the start of a session for phase status, persistent
traps, and the latest session's "Suggested next sub-targets". At the
end of a session, **rewrite** the Latest session entry, add a
one-line entry to the Earlier sessions log, and merge any
still-relevant trap into the Persistent traps section. Keep the file
short — see `SKILL.md`'s "Session recap" section for length budget.

--------------------------------------------------------------------------------

## Persistent traps & invariants (cross-session)

These survive across sessions. Add to this list when a trap is
re-relevant (i.e. you'd warn a future session about it); fold it
back into a session entry only if it's purely historical.

### Disk + tooling

- **Disk lint cache at `~/.cache/panache/`** serves stale
  `undefined-anchor` (and other linter rule) results even after
  `cargo build`. Symptoms: unit tests pass, `panache lint` keeps
  emitting old diagnostics, `eprintln!` from changed code never
  fires. Fix: `rm -rf ~/.cache/panache/` (or
  `cache.enabled = false` in `panache.toml`). Validate via unit
  tests first; treat CLI as downstream.
- **Conformance comparison is whitespace-insensitive**:
  `normalize_native` collapses pandoc's pretty-printed multi-line
  block output to single-line. Visual diffs are misleading.

### Parser shape & losslessness

- **HTML_ATTRS is the structural pattern; never add synthetic
  tokens.** Expose attributes by tokenizing existing source bytes at
  finer granularity (split TEXT into
  `TEXT + WHITESPACE + HTML_ATTRS{TEXT} + TEXT`). Synthetic tokens
  break the tree-text-equals-input invariant.
- **Use source-byte slices, never literal strings, when emitting
  TEXT tokens** for HTML. `"<div"` literal vs `&rest[..4]` was the
  root of the `<DIV>` losslessness regression. Case-insensitive
  prefix matches give a false sense of byte-identity.
- **Same-line `<div>foo</div>` is ONE `HTML_BLOCK_TAG`**, not open
  + content + close. The close `</div>` lives inside a TEXT child
  of the open tag. Any naive `strip_suffix('>')` grabs the wrong
  `>`. Scan to the first **unquoted** `>` (see
  `parse_html_tag_attributes`).
- **Quoted attribute values can hide `<` and `>`.** Tag-bracket
  scanners must thread quote state across line boundaries; don't
  reset per-line. `count_tag_balance`, `find_multiline_div_open_end`,
  `pandoc_html_open_tag_closes` do this right.
- **Incomplete open tags caused projector infinite recursion.**
  `<embed\n`, `<div\n`, `<table\n` etc. (no `>` anywhere) were
  recognized as `RawBlock` under Pandoc, but pandoc-native treats
  them as paragraph text. The projector's `flush_html_block_tail_text`
  then reparsed the same bytes and re-emitted the same HTML_BLOCK,
  recursing forever. Fix: gate Pandoc BlockTag recognition on
  `pandoc_html_open_tag_closes(lines, line_pos, bq_depth)` in
  `block_dispatcher.rs::detect_prepared`. Multi-line opens still
  work because the helper scans subsequent lines (across blank
  lines, threading quotes) for an unquoted `>`. CommonMark must
  remain liberal: `<table\n` (no `>`) is a valid CM type-6
  RawBlock.
- **Self-closing `<tag/>` doesn't bump depth.** Depth-aware close
  matchers must check `bytes[j-1] == b'/'` at the closing `>`.
- **`input.lines()` strips newlines**; for losslessness-asserting
  parser tests use
  `crate::parser::utils::helpers::split_lines_inclusive` to build
  `lines: Vec<&str>`.
- **`HtmlBlockType::BlockTag` is `Box<dyn Any>`-roundtripped via
  the block dispatcher.** Adding a field works automatically;
  cargo's E0063 errors point at every literal site that needs
  updating.

### Pandoc tag categorization

- **Pandoc has THREE tag sets, not one**: strict block
  (`PANDOC_BLOCK_TAGS`), inline-block non-void
  (`PANDOC_INLINE_BLOCK_TAGS`), inline-block void
  (`PANDOC_VOID_BLOCK_TAGS`). Each requires distinct handling — the
  strict set always splits, the non-void set follows
  `inline_pending` and lifts as matched-pair, the void set follows
  `inline_pending` and emits a single RawBlock per instance. Source
  of truth: `pandoc/src/Text/Pandoc/Readers/HTML/TagCategories.hs`
  + `Readers/HTML.hs::isBlockTag`/`isInlineTag`.
- **`eitherBlockOrInline` is context-dependent.** Mirroring needs
  BOTH parser-side `cannot_interrupt` (don't break running paragraph)
  AND projector-side `inline_pending` tracking (don't split mid-text).
  Either alone is insufficient.
- **CommonMark and Pandoc `blockHtmlTags` lists differ in BOTH
  directions** by ~15 tags. Don't merge them. The parser's
  `is_commonmark` flag gates which list runs; the projector only
  runs under Pandoc and uses `is_pandoc_block_tag_name` directly.
- **Closing forms must be excluded from the block-start
  recognizer.** `<button>` opens a block; `</button>` does not (it
  goes inline). Mirrors pandoc's `htmlTag isBlockTag` which only
  matches open tags.
- **`<script>` is in `eitherBlockOrInline` AND `blockHtmlTags`.**
  Verbatim handling fires first via `VERBATIM_TAGS`; don't add
  `script` to `PANDOC_INLINE_BLOCK_TAGS`. Likewise `<pre>`,
  `<style>`, `<textarea>` membership in `PANDOC_BLOCK_TAGS` is
  harmless — the verbatim arm fires first.

### Projector tag splitting

- **`split_html_block_by_tags` walks bytes, not tokens.** It is
  depth-unaware (Phase 5 work for the few cases that need it) and
  context-tracked via `inline_pending`. Don't try to "merge" with
  `find_matching_close` (the smart-quote bracket scanner) — same
  name, different inputs.
- **`inline_pending` resets on consecutive newlines (≥ 2).** A
  blank line restarts pandoc's block parser; in our byte walker
  that's `\n\n`. Don't substitute "byte == whitespace" — single
  trailing whitespace shouldn't reset.
- **Inter-tag text demotes Para→Plain when butted against the next
  tag**; tail text does NOT demote. Use `flush_html_block_text`
  (inter-tag) vs `flush_html_block_tail_text` (end-of-block).
  Uniform demotion silently breaks `<form>\nfoo\n` and
  `<embed src="x"> trailing` shapes.
- **Plain/Para signal for `<div>` recursive reparse is
  `</div>`-side, not `<div>`-side**: `close_butted = byte_at(close_start - 1) != '\n'`.
  Demotion applies to the LAST block only, regardless of how many
  precede it.
- **`try_div_html_block` requires the WHOLE content to be a single
  `<div>...</div>`** with optional surrounding whitespace. Pass an
  exact `<div>...</div>` slice when calling on a sub-range.

### Refs / footnotes / heading-id resolution

- **`parse_pandoc_blocks` swaps in an inner `RefsCtx`** for the
  recursive `<div>` reparse (and any other call site). The swap
  belongs in `parse_pandoc_blocks` itself, not at call sites.
- **`build_refs_ctx` mutates `REFS_CTX` mid-build** (stages
  cite-num/example-num maps before the heading pre-pass). When
  swapping for an inner reparse, save outer FIRST (`mem::take`),
  THEN call `build_refs_ctx`, THEN install the result.
- **`heading_id_by_offset` is offset-keyed, not slug-keyed.** The
  inner CST's offsets are zero-based and don't intersect the
  outer's offset space. Tempting wrong fix: copy outer
  `heading_ids` into inner. Right fix: build a fresh inner ctx and
  optionally inherit cross-boundary refs/footnotes via
  `build_refs_ctx_inherited`.
- **`fenced_div` does NOT use `parse_pandoc_blocks`** — it walks
  the structural CST via `collect_block`. Fenced divs already
  resolve through the outer ctx; don't generalize the swap to
  fenced divs.
- **`AttributeNode::can_cast` accepts `HTML_ATTRS`**; the existing
  salsa walk picks up `<div id>` / `<span id>` automatically. No
  parallel salsa walk for HTML attrs.

### Out of scope / known divergences

- **`<!ENTITY x "y">` projects `Str "\"y\">"`** where pandoc emits
  `Quoted DoubleQuote [Str "y"]`. Smart-quote / Quoted feature
  gap; not html-conformance.
- **Outer-wins-over-inner ref-conflict**: pandoc's rule is
  document-order-first; we have inner-wins. No corpus exercises
  this; deferred.
- **Cross-boundary cite numbering** for `<div>` recursive reparse
  similarly deferred.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Complete** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Complete** (2026-05-08) |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Complete** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10) |
| 4 | Comments, PIs, declarations, CDATA projection | **Complete** (2026-05-08); type-4 CM lowercase still gappy |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Mostly complete** — depth-aware nested div, Plain/Para promotion, refs inheritance, projector-level splitter all landed; outer-matched-pair-with-inner-split-conflict still gappy |

Multi-line `<div>` open-tag structural HTML_ATTRS lift landed
(2026-05-09). Multi-line void open-tag still falls back to opaque
HTML_BLOCK.

--------------------------------------------------------------------------------

## Latest session — 2026-05-10 (incomplete open-tag recursion fix)

**html pass count: 113 → 117** (+4 new corpus cases). **Workspace:
0 → 0 failing.** **Total pandoc conformance: 305/305 → 309/309.**
**New parser fixtures: 2** (paired incomplete open tag
Pandoc/CommonMark).

### What landed

Triage of the previous session's "next #2" (multi-line void open
tags) surfaced a pre-existing severe bug: incomplete HTML open
tags (`<embed\n`, `<div\n`, `<table\n`, `<iframe\n` etc. with no
`>` anywhere) crashed the projector with a stack overflow under
Pandoc dialect. Pandoc-native treats them as paragraph text;
recognizing them as `RawBlock` made the projector reparse the same
bytes and infinite-recurse. The bug also fired for blockquoted
inputs (`> <embed`).

1. New `pandoc_html_open_tag_closes(lines, start_pos, bq_depth)`
   helper in `parser/blocks/html_blocks.rs`. Scans from the open
   tag's `<` forward for the first unquoted `>`; threads quote state
   across line boundaries; tolerates blank lines (matching pandoc's
   `htmlTag` behavior).
2. Wired into `block_dispatcher.rs::detect_prepared`: under Pandoc
   dialect, when `try_parse_html_block_start` returns `BlockTag`,
   reject the recognition if the open tag never closes. CommonMark
   stays liberal — its type-6 `<table\n` (no `>`) IS a valid
   RawBlock, so the gate is dialect-specific.
3. New unit test `test_pandoc_html_open_tag_closes` covering
   single-line complete, multi-line complete, quoted-`>` immunity,
   blank-line tolerance, and the no-`>` rejection cases.

4 new corpus cases (0306–0309) covering void (`<embed`),
strict-block (`<div`, `<table`), and inline-block (`<iframe`) open
tags with no `>` — all expected to emit `Para [Str "<tagname"]` in
pandoc-native. 2 new paired parser fixtures
(`html_block_incomplete_open_{pandoc,commonmark}`) pin the
dialect-divergent CST: Pandoc → 3 paragraphs; CommonMark → 1
paragraph (`<embed`, not in BLOCK_TAGS) + 2 HTML_BLOCKs (`<div`,
`<table` are CM type-6 starters).

The previous session's "next #2" (multi-line void open-tag
recognition: `<embed\n  src="x">` → single RawBlock instead of
two Paras) is **still gappy** — it needs a parallel multi-line
open-tag detection like `<div>` has, plus a void-tag wrapper
emit path. Out of scope for this session.

### Files in committable diff

- Parser-shape: `parser/blocks/html_blocks.rs` (~50 lines net),
  `parser/block_dispatcher.rs` (~16 lines).
- Corpus: 4 new dirs under `corpus/0306..0309-…/`.
- Allowlist + report regenerated.
- 2 new parser fixtures + snapshots, registered in
  `golden_parser_cases.rs`.

No projector, salsa, formatter, linter, LSP, or other host-side
changes.

### Suggested next sub-targets, ranked

1. **Multi-line void open tags** (`<embed\n  src="x">`).
   Today panache recognition rejects (good — no recursion) but the
   structural lift is missing: pandoc emits a single RawBlock
   spanning the multi-line tag; we emit two paragraphs. Generalize
   `find_multiline_div_open_end` to a tag-name-parametrized
   `find_multiline_open_end(lines, start_pos, tag_name)` and call
   it from the void-tag branch in `try_parse_html_block_start`'s
   call site (i.e. from `parse_html_block` like the `<div>`
   path). Strict-block tags like `<table\n  border="1">` likely
   want the same treatment.
2. **`<video>\n<source>\nfallback\n</video>` outer-wins-on-conflict**
   (still deferred; no corpus case yet — needs a blocked.txt entry
   first).
3. **Audit `parse_html_attrs` and `find_matching_html_close` for
   literal-byte hazards** (still on the list from earlier sessions).
4. **Outer-wins-on-conflict for inherited refs/footnotes** (still
   deferred — no corpus exercises it).

### New trap (folded into Persistent traps)

- Pre-fix recursion mechanism: parser emits HTML_BLOCK for `<embed\n`;
  projector's `flush_html_block_tail_text` calls `parse_pandoc_blocks`
  which re-parses to the same HTML_BLOCK; emit_html_block recurses.
  Persistent-traps entry covers it.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-10 — Phase 3 void-element `eitherBlockOrInline` lift
  (`<embed>`, `<area>`, `<source>`, `<track>`) — html 105 → 113 —
  new `PANDOC_VOID_BLOCK_TAGS` + `closes_at_open_tag: bool`;
  projector void-tag branch with `inline_pending` rule; split
  `flush_html_block_text` (demotes) vs `flush_html_block_tail_text`
  (preserves Para).
- 2026-05-09 — Phase 3 `eitherBlockOrInline` non-void lift (`<iframe>`,
  `<button>`, `<video>`, `<del>`, etc.) — html 94 → 105 — context-aware
  projector with `inline_pending` flag + parser-side
  `cannot_interrupt`; blocked iframe (#287) unblocked.
- 2026-05-09 — Phase 3 corpus expansion (HTML5 sectioning + grouping:
  `<header>`, `<footer>`, `<main>`, `<details>`, `<figure>`,
  `<figcaption>`, `<nav>`) — html 87 → 94 — pure corpus growth + doc
  comment update; documented `eitherBlockOrInline` gap.
- 2026-05-09 — Phase 5 audit pivoted to `<DIV>` losslessness fix —
  html 87 → 87 — `emit_div_open_tag_tokens` had literal `"<div"`
  instead of source bytes; one-line fix + uppercase paired fixture.
  Projector cleanup deferred (low value).
- 2026-05-09 — Phase 3 dialect-divergent `blockHtmlTags`
  (`<dialog>`/`<canvas>` etc.) — html 80 → 87 — split CM/Pandoc
  block-tag lists; 7 new corpus cases.
- 2026-05-09 — Phase 5 Plain/Para promotion rule for `<div>`
  recursive reparse — html 76 → 80 — projector-only;
  `close_butted = byte_at(close_start - 1) != '\n'`; demote LAST
  block only.
- 2026-05-09 — Phase 1 multi-line `<div>` open-tag
  HTML_ATTRS structural lift — html 75 → 76 — per-line
  `HTML_ATTRS` nodes (not one big spanning node); quote state threads
  across line boundaries.
- 2026-05-09 — Phase 5 cross-boundary `RefsCtx` inheritance for
  outer→inner refs/footnotes/heading-slugs — html 72 → 75 — new
  `build_refs_ctx_inherited`; `parse_pandoc_blocks` calls it with
  `Some(&outer)`; AST gains `Clone`.
- 2026-05-09 — Phase 5 inner-`RefsCtx` for `parse_pandoc_blocks`
  recursive reparse — html 62 → 72 — heading auto-ids, ref defs,
  footnote defs inside `<div>` resolve in inner ctx; outer ctx
  saved via `mem::take` and restored.
- 2026-05-08 — Phase 5 depth-aware nested `<div>` close scan
  (case 199 unblocked) — html 57 → 62 — `count_tag_balance` walks
  same-name opens/closes; new `depth_aware` field on `BlockTag`;
  CM verbatim keeps first-close.
- 2026-05-08 — Phase 5/6 projector-level `markdown_in_html_blocks`
  for non-sectioning block tags — html 47 → 57 — byte-aware
  `split_html_block_by_tags`; new `find_matching_html_close`,
  `flush_html_block_text`, `extract_html_tag_name`.
- 2026-05-08 — CommonMark type-4 lowercase declaration recognition
  — html 47 → 47 (CM-side fix; no Pandoc corpus impact) — paired
  parser fixture.
- 2026-05-08 — Phase 4 follow-up: gate type-4/type-5 HTML blocks
  off under Pandoc dialect — html 39 → 47 — `<!DOCTYPE>`/`<![CDATA>`
  fall through to paragraph parsing; `try_parse_inline_html` gained
  `dialect: Dialect` parameter.
- 2026-05-08 — Phase 4 comments + processing instructions corpus
  pin — html 27 → 39 — pure corpus growth; declaration/CDATA
  parser-shape gap noted.
- 2026-05-08 — Phase 3 sectioning + verbatim negative-space pin
  (`<section>`, `<article>`, `<aside>`, `<nav>`, `<pre>`, `<style>`,
  `<script>`, `<textarea>`) — html 17 → 27 — pure corpus growth.
- 2026-05-08 — Phase 2 `<span>` inline lift — html 9 → 17 —
  `INLINE_HTML_SPAN` retag of `BRACKETED_SPAN`; attribute region
  restructured from `SPAN_ATTRIBUTES` token to `HTML_ATTRS` node.
  `<span>` was already lifting; corrected the misleading "INLINE_HTML"
  starting-state claim from Phase 1's RECAP.
- 2026-05-08 — Phase 1 `<div>` block lift (issue #263 closed) —
  html 0 → 9 — `HTML_BLOCK_DIV` wrapper retag + `HTML_ATTRS`
  open-tag tokenization; `AttributeNode::can_cast(HTML_ATTRS)` so
  salsa walk picks up `<div id>` automatically; nested-div blocked
  as Phase 5 target.
