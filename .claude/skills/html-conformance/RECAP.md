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
  reset per-line. `count_tag_balance`, `find_multiline_open_end`,
  `pandoc_html_open_tag_closes` do this right.
- **Multi-line open-tag close branches diverge by tag class.** The
  `same_line_closed` short-circuit assumes single-line; void-tag
  multi-line opens take a separate early-exit returning
  `end_line_idx + 1` BEFORE the regular close-marker loop. Without
  the explicit branch the parser would scan content lines for a
  closing tag that doesn't exist (void tags have none) and run
  off the document. Likewise `same_line_closed` must guard
  `multiline_open_end.is_none()`.
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
- **Closing forms of strict-block, verbatim, inline-block, and void
  tags ALL ARE block starts under Pandoc.** Pandoc's `htmlBlock
  isBlockTag` matches both directions for any tag in
  `blockHtmlTags ∪ verbatimTags ∪ eitherBlockOrInline`. Routing in
  the parser: each category emits `BlockTag { closes_at_open_tag:
  true }` so the block ends on the open line. The dispatcher's
  `cannot_interrupt` gate keys ONLY on inline-block + void tag
  names — strict-block (`</p>`, `</nav>`, `</section>`) and verbatim
  (`</pre>`, `</style>`, `</script>`, `</textarea>`) closes get
  `YesCanInterrupt` and DO interrupt running paragraphs (matches
  pandoc). Inline-block / void closes follow `cannot_interrupt`
  semantics and stay inline inside running paragraphs
  (`foo\n</video>` → `Para[foo, SB, RI</video>]`). Earlier recap
  claims that "closing forms must be excluded" were wrong on all
  counts.
- **`<script>` is in `eitherBlockOrInline` AND `blockHtmlTags`.**
  Verbatim handling fires first via `VERBATIM_TAGS`; don't add
  `script` to `PANDOC_INLINE_BLOCK_TAGS`. Likewise `<pre>`,
  `<style>`, `<textarea>` membership in `PANDOC_BLOCK_TAGS` is
  harmless — the verbatim arm fires first.
- **`<style>` and processing instructions cannot interrupt under
  Pandoc.** Pandoc-native: `foo\n<style>x</style>` and
  `foo\n<?...?>` stay as a single `Para` with the tag(s) as
  `RawInline`. Panache currently splits both into RawBlock
  (post-2026-05-10 with PARAGRAPH→PLAIN: `[Plain[foo],
  RawBlock<style>...]`). Fix: extend `cannot_interrupt` in
  `HtmlBlockParser::detect_prepared` to include the `<style>`
  BlockTag and `HtmlBlockType::ProcessingInstruction` under
  Pandoc dialect. (`<script>` must keep splitting — pandoc *does*
  treat it as a strict block.)

### Projector tag splitting

- **`split_html_block_by_tags` walks bytes, not tokens.** It is
  depth-unaware (Phase 5 work for the few cases that need it) and
  context-tracked via `inline_pending`. Don't try to "merge" with
  `find_matching_close` (the smart-quote bracket scanner) — same
  name, different inputs.
- **Matched-pair lift for `<video>...</video>` must abandon when
  interior opens with a void block tag at column 0.** Pandoc-native
  emits per-tag (`<video>` RB, `<source>` RB, Para[fallback, SB,
  RawInline</video>]) — not a balanced lift. Helper
  `interior_starts_with_void_block_tag` peeks past leading
  newlines/whitespace; on hit, the open tag emits as a single
  RawBlock and the closing `</video>` falls into the trailing
  paragraph reparse as RawInline. Indentation before the void tag
  doesn't save the lift (pandoc abandons even with 4-space indent).
- **Inline-block open with no matched close must emit as RawBlock
  at fresh-block.** Falling through to `inline_pending=true` causes
  the trailing tail-text reparse to recurse on the same `<video>...`
  bytes (parser still recognizes the open tag, projector splits it
  again, …) → stack overflow. The same `interior_starts_with_void`
  bail and the no-match bail share the single-tag emit path.
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
- **Top-level Para→Plain demotion at HTML strict-block / verbatim
  adjacency: LANDED 2026-05-10.** Parser-side fix in
  `Parser::close_paragraph_as_plain_if_open` +
  `html_block_demotes_paragraph_to_plain`, wired at the
  YesCanInterrupt branch in `core.rs`. Gated on `Dialect::Pandoc` +
  `parser_name == "html_block"` + `HtmlBlockType::BlockTag`. CST
  emits `PLAIN` instead of `PARAGRAPH`; projector trivially maps
  each. Don't reintroduce the projector-side demotion (reverted
  earlier the same day).

### Projector-as-second-stage-parser smell (architectural)

The pandoc-AST projector at `crates/panache-parser/src/pandoc_ast.rs`
is a **test-only diagnostic** for CST shape, not a runtime artifact.
Phases 1/5 landed structural retags (`HTML_BLOCK_DIV`,
`INLINE_HTML_SPAN`) but stopped short of lifting inner block content
into structural CST children. Today the projector still re-runs the
markdown parser on HTML block bodies via `parse_pandoc_blocks` /
`split_html_block_by_tags` / `flush_html_block_text` /
`flush_html_block_tail_text` / `try_div_html_block`. That makes the
conformance harness pass while the CST stays opaque — consumers
(linter, salsa, LSP, formatter) walking the CST don't see the
structural decisions pandoc encodes. **The path forward is parser
work** (lift inner blocks into CST children, retag PARAGRAPH→PLAIN
when appropriate, etc.); each lift collapses a chunk of projector
compensation into a trivial CST walk. Defensible reparses (table
cells via `parse_grid_cell_text` / `parse_cell_text_inlines`) match
how pandoc itself sub-parses cell content and can stay.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` gets `PARAGRAPH` / `LIST` / etc. as direct children; `split_html_block_by_tags` / `flush_html_block_*` / `parse_pandoc_blocks` collapse into trivial CST walks; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **Fix #1 landed (2026-05-10)** — `PARAGRAPH→PLAIN` retag at YesCanInterrupt for HTML BlockTag under Pandoc; +5 conformance (132 → 137 html). Fixes #2-#4 from AUDIT.md still pending: walk structural CST for `<div>` open-tag attrs, lift inner blocks into CST children, full `HTML_BLOCK` body structural split. |

Multi-line `<div>` open-tag structural HTML_ATTRS lift landed
(2026-05-09). Multi-line void open-tag now lifts via
`find_multiline_open_end` + simple per-line TEXT/NEWLINE emission
(2026-05-10). Inline-block / void closing forms (`</video>`,
`</embed>`) start single-line `RawBlock`s under Pandoc (2026-05-10).
Strict-block / verbatim closing forms (`</p>`, `</nav>`, `</section>`,
`</pre>`) likewise lift under Pandoc, with `closes_at_open_tag: true`
and CAN interrupt a running paragraph (no `cannot_interrupt` gate)
(2026-05-10).

--------------------------------------------------------------------------------

## Latest session — 2026-05-10 (Phase 6 fix #1 — PARAGRAPH→PLAIN retag at HTML strict/verbatim adjacency)

Implemented audit fix #1: parser retags PARAGRAPH→PLAIN when an open
paragraph is interrupted by an HTML strict-block / verbatim block tag
under Pandoc dialect. Replicates the 2026-05-10-reverted projector
demotion as a single-site parser decision; projector unchanged.

Pass count: 324 → 329 total (+5), html 132 → 137 (+5).

### What landed

- New `Parser::close_paragraph_as_plain_if_open` + helper
  `html_block_demotes_paragraph_to_plain(block_match)` in
  `crates/panache-parser/src/parser/core.rs`. Wired at the
  YesCanInterrupt branch (line ~2978) — the only site that pops a
  paragraph due to interruption.
- Helper gates on `Dialect::Pandoc`, `parser_name == "html_block"`,
  and `HtmlBlockType::BlockTag {..}`. By construction at this site
  the BlockTag is strict-block (`PANDOC_BLOCK_TAGS`) or verbatim
  (`VERBATIM_TAGS`) — inline-block / void / Type7 / comments take
  the `cannot_interrupt` path and never reach here.
- Corpus: 5 new cases (0325-0329) — paragraph + strict-open,
  paragraph + strict-close, paragraph + div, paragraph + verbatim,
  multi-line paragraph + strict-open. New allowlist section
  `# html-block (paragraph PARAGRAPH→PLAIN demotion …)`.
- Parser fixtures (paired CM/Pandoc): pinned dialect divergence —
  Pandoc emits PLAIN, CommonMark keeps PARAGRAPH.
- Formatter golden (`html_block_paragraph_demote`) — idempotency
  holds; PLAIN at top level formats identically to PARAGRAPH (text
  bytes unchanged), and pass1 == pass2.

### Suggested next sub-targets

Audit's ranked list, items still pending:

1. **Fix #2 — `html_div_block` walks structural CST for open-tag
   attrs** (small projector cleanup, no conformance impact). Sets
   the precedent for #3 by removing one of
   `try_div_html_block`'s two byte walks. Pure
   `pandoc_ast.rs` change.
2. **Fix #3 — lift `<div>` inner blocks into structural CST
   children** (Phase 6 proper, medium). Collapses
   `parse_pandoc_blocks` recursive reparse, `close_butted` rule,
   cross-boundary `RefsCtx` swap. Linter/salsa/LSP can finally
   see structural children of a `<div>`.
3. **Fix #4 — full HTML_BLOCK body structural split** (large;
   defer until #3 lands). Eliminates `split_html_block_by_tags`,
   both flush helpers, `interior_starts_with_void_block_tag`,
   `find_matching_html_close*`, `inline_pending` flag.
4. **`<style>` cannot_interrupt under Pandoc** — discovered this
   session. Pandoc-native: `foo\n<style>x</style>\n` →
   `[Para[foo, SoftBreak, RawInline<style>, Str x, RawInline</style>]]`
   (no split). Panache splits and (post-fix) emits
   `[Plain[foo], RawBlock<style>x</style>]`. Same gap exists for
   processing instructions (`<?...?>`). Belongs in
   `HtmlBlockParser::detect_prepared` — extend `cannot_interrupt`
   in Pandoc dialect to include `<style>` BlockTag and
   `HtmlBlockType::ProcessingInstruction`. Small parser change,
   needs corpus pin first.

### Files in committable diff

- `crates/panache-parser/src/parser/core.rs` — new methods +
  YesCanInterrupt wire-up.
- `crates/panache-parser/tests/pandoc/{allowlist,report}.txt`,
  `docs/development/pandoc-report.json` — regenerated.
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/`
  — 5 new cases (0325-0329).
- `crates/panache-parser/tests/fixtures/cases/` + `tests/snapshots/`
  + `tests/golden_parser_cases.rs` — 3 new parser fixtures (paired
  CM/Pandoc) + snapshots.
- `tests/fixtures/cases/html_block_paragraph_demote/` +
  `tests/golden_cases.rs` — formatter golden + wire-up.
- `.claude/skills/html-conformance/RECAP.md` — this entry.

### New traps

`<style>` and PI gap noted above; folded into Persistent traps as
"Pandoc tag categorization" addendum.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-10 — Projector audit; AUDIT.md landed — html 132 → 132 —
  inventoried `pandoc_ast.rs` (5,696 lines), classified each
  reparse / byte walker / context-dependent decision as defensible
  vs CST gap; produced ranked parser-side fix list (#1
  PARAGRAPH→PLAIN, #2 `html_div_block` structural walk, #3
  `<div>` inner-block lift, #4 full HTML_BLOCK split).
- 2026-05-10 — Course correction; aborted projector Para→Plain
  demotion reverted — html 132 → 132 — projector compensation
  defeats the diagnostic; added "What this skill is NOT" to
  SKILL.md and Phase 6 row to track structural-lift work.
- 2026-05-10 — Strict-block + verbatim closing-form lift (`</p>`,
  `</nav>`, `</section>`, `</pre>`) — html 126 → 132 — new branch in
  `try_parse_html_block_start` accepts Pandoc closes for `PANDOC_BLOCK_TAGS
  ∪ VERBATIM_TAGS` with `closes_at_open_tag: true`; closes interrupt
  running paragraphs.
- 2026-05-10 — Inline-block close lift + matched-pair-abandons-on-void-interior
  — html 122 → 126 — `closes_at_open_tag: true` for closing forms;
  `interior_starts_with_void_block_tag` helper in projector; single-RawBlock
  emit closes recursive tail-text reparse.
- 2026-05-10 — Multi-line void open-tag recognition (`<embed\n …>`) —
  html 117 → 122 — generalized `find_multiline_open_end` over tag name;
  void early-exit returns `end_line_idx + 1`.
- 2026-05-10 — Incomplete open-tag projector recursion fix — html 113 →
  117 — `pandoc_html_open_tag_closes` gate in `block_dispatcher`; CM
  type-6 stays liberal.
- 2026-05-10 — Phase 3 void-element `eitherBlockOrInline` lift
  (`<embed>`, `<area>`, `<source>`, `<track>`) — html 105 → 113 — new
  `PANDOC_VOID_BLOCK_TAGS` + `closes_at_open_tag`; projector void-tag
  branch with `inline_pending` rule; split `_text` (demotes) vs
  `_tail_text` (preserves) helpers.
- 2026-05-09 — Phase 3 `eitherBlockOrInline` non-void lift (`<iframe>`,
  `<button>`, `<video>`, `<del>`, …) — html 94 → 105 — context-aware
  projector with `inline_pending` + parser `cannot_interrupt`; #287
  unblocked.
- 2026-05-09 — Phase 3 HTML5 sectioning + grouping corpus (header,
  footer, main, details, figure, figcaption, nav) — html 87 → 94 —
  pure corpus growth.
- 2026-05-09 — `<DIV>` losslessness fix + dialect-divergent
  `blockHtmlTags` split — html 80 → 87 — source-byte fix in
  `emit_div_open_tag_tokens`; CM/Pandoc lists split.
- 2026-05-09 — Phase 5 `<div>` Plain/Para promotion + multi-line
  open-tag HTML_ATTRS lift + cross-boundary refs inheritance — html
  62 → 80 — `build_refs_ctx_inherited` + inner `RefsCtx` swap via
  `mem::take`.
- 2026-05-08 — Phase 5 depth-aware nested `<div>` close scan +
  projector `markdown_in_html_blocks` for non-sectioning block tags —
  html 47 → 62 — `count_tag_balance`, `depth_aware`; byte-aware
  `split_html_block_by_tags`.
- 2026-05-08 — Phase 4 declarations / CDATA / comments / PIs — html
  27 → 47 — pandoc gate on type-4/type-5 + corpus pin.
- 2026-05-08 — Phase 3 sectioning + verbatim negative-space pin — html
  17 → 27 — pure corpus growth.
- 2026-05-08 — Phase 2 `<span>` inline lift — html 9 → 17 —
  `INLINE_HTML_SPAN` retag + `HTML_ATTRS` restructure.
- 2026-05-08 — Phase 1 `<div>` block lift (issue #263 closed) — html
  0 → 9 — `HTML_BLOCK_DIV` retag + `HTML_ATTRS` open-tag tokenization.
