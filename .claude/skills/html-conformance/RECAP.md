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
- **`<style>`, PIs, `</script>`, and `<script type="math/tex…">`
  cannot interrupt a paragraph under Pandoc; `<pre>`/`<script>` open
  without math/tex/`<textarea>` DO** (LANDED 2026-05-10 / 2026-05-11).
  The non-interrupt set mirrors pandoc's `isInlineTag` predicate
  (`pandoc/src/Text/Pandoc/Readers/HTML.hs`):
  - `<style>` open AND close are SPECIAL-CASED to always be inline
    (commit fixing pandoc issue #10643).
  - `</script>` close is similarly special-cased to always be inline.
  - `<script>` open is inline ONLY when the `type` attribute starts
    with `math/tex` (case-insensitive prefix; e.g. `math/tex`,
    `math/tex; mode=display`). Every other `<script>` open is a
    `RawBlock`.
  - PIs (`<? … ?>`) match `T.take 1 name == "?"`.
  - Comments are always inline.
  - Pandoc's `eitherBlockOrInline` set (audio, button, iframe, …,
    plus void area/embed/source/track) returns True from
    `isInlineTag` because those tags are NOT in `blockTags`.
  Earlier RECAP entries claimed `<style>` was "the lone verbatim
  tag NOT in `blockHtmlTags` (verbatimHtmlBlocks only)" — wrong;
  pandoc's `blockHtmlTags` does include `style` and `textarea`. The
  behavior difference comes from `isInlineTag`'s special cases, not
  tag-set membership. Fix: `cannot_interrupt` in
  `HtmlBlockParser::detect_prepared` includes
  `HtmlBlockType::ProcessingInstruction`, `BlockTag`s where
  `tag_name == "style"`, `BlockTag`s where
  `is_closing && tag_name == "script"`, and `BlockTag`s where
  `!is_closing && tag_name == "script" && is_math_tex_script_open(ctx.content)`
  under `Dialect::Pandoc`. The math/tex helper inspects only
  `ctx.content` (single-line opens); multi-line `<script\n type="math/tex">`
  opens are an edge case not yet exercised by the corpus. Required
  adding an `is_closing: bool` field to `HtmlBlockType::BlockTag`
  (carries through every literal site). CommonMark stays liberal —
  paired CM/Pandoc parser fixtures pin any divergence.

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
- **HTML blocks (any tag) inside blockquotes need projector marker-
  strip.** The parser keeps `BLOCK_QUOTE_MARKER + WHITESPACE` as
  structural CST tokens inside `HTML_BLOCK_CONTENT` and the close
  `HTML_BLOCK_TAG` for losslessness. Calling `node.text()` on
  `HTML_BLOCK` / `HTML_BLOCK_DIV` returns those markers as literal
  bytes; feeding them to `parse_pandoc_blocks` /
  `split_html_block_by_tags` / `try_div_html_block` re-recognizes
  the `> ` prefixes as a nested blockquote (or for verbatim
  `<pre>...</pre>`, leaves literal `>` chars in the emitted
  RawBlock). Always use `collect_html_block_text_skip_bq_markers`
  in `html_div_block` and `emit_html_block` (the two byte-reparse
  entry points). Walker collapses each `BLOCK_QUOTE_MARKER` plus
  one immediately-following `WHITESPACE` token; handles arbitrary
  nesting depth (`> > <div>`). Don't reintroduce `node.text()` on
  these paths until the parser-side structural lift inside bq lands.
- **Projector RawBlock emission of lifted open `HTML_BLOCK_TAG`
  must canonicalize when structural `HTML_ATTRS` are present.**
  Multi-line open tags carry literal source bytes
  (`<form\n  id="x"\n  class="y">`) — feeding these to RawBlock
  diverges from pandoc-native's canonical single-line form
  (`<form id="x" class="y">`). `normalize_native` preserves
  whitespace inside `"..."` string literals, so the conformance
  harness sees the divergence. Use `open_tag_raw_block_text` in
  `pandoc_ast.rs`: walk `children_with_tokens`, take the leading
  `<tagname` TEXT, append each `HTML_ATTRS`'s trimmed text
  separated by single spaces, append `>`. Single-line opens
  without HTML_ATTRS (`<form>`, `</form>`) keep their literal
  text. Don't substitute `node.text()` on the open-tag emission
  path even if the literal looks correct — it diverges on
  multi-line and on non-canonical whitespace within attribute
  regions.

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
  salsa walk picks up `<div id>` / `<span id>` automatically. As of
  2026-05-11 (Fix #4 shape-lift extension) the same walk also picks up
  non-div strict-block tag ids (`<section id="x">`, `<form id="x">`,
  `<p id="x">`, etc.) because the parser now emits `HTML_ATTRS` for
  those tags too. Diverges from pandoc-native (which keeps them as
  RawBlock without lifting attrs), but matches user intent for
  anchor-link resolution — the linter no longer false-positives
  `undefined-anchor` against `<section id>`. No parallel salsa walk
  for HTML attrs.

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

The projector at `crates/panache-parser/src/pandoc_ast.rs` is the
public `panache_parser::to_pandoc_ast` API; consumers of structural
HTML decisions (linter, salsa, LSP, formatter) walk the CST, not
the projector. Phases 1/5 landed structural retags
(`HTML_BLOCK_DIV`, `INLINE_HTML_SPAN`); Phase 6 lifted inner content
of all non-bq `<div>` shapes (clean / messy / empty) AND non-div
Pandoc strict-block tags in the clean shape (`<form>...</form>`,
`<section>...</section>`, `<table>...</table>`, etc.) into CST
children. The projector still re-runs the markdown parser on opaque
`HTML_BLOCK` bodies for the remaining shapes (same-line / open-
trailing / butted-close / bq-wrapped non-div, matched-pair inline-
block `<video>...</video>`, multi-line opens) via
`parse_pandoc_blocks` / `split_html_block_by_tags` /
`flush_html_block_*` / `try_div_html_block`. **The path forward is
parser work** — extend the lift to the remaining shapes (butted-
close → same-line → bq-wrapped → matched-pair inline-block) and
then prune the byte walkers. Defensible reparses (table cells via
`parse_grid_cell_text` / `parse_cell_text_inlines`) mirror pandoc
and stay.

### Structural lift (Fix #3 / Fix #4 family)

- **Recursive parse uses `parse_with_refdefs`, not `parse`.** When
  doing an inner recursive parse for a structural lift, call
  `crate::parser::parse_with_refdefs(inner_text, opts, outer_refdefs)`
  (or thread the outer config's `refdef_labels` through). `parse`
  re-runs `populate_refdef_labels` on JUST the inner text, hiding
  outer refdefs from inner reference links.
- **Lifted HTML_BLOCK / HTML_BLOCK_DIV MUST route to the structural
  walk, never the byte path.** `collect_block` routes
  `HTML_BLOCK_DIV` to `html_div_block` (not `emit_html_block`);
  `emit_html_block` internally routes lifted HTML_BLOCKs to
  `emit_html_block_structural` (not `split_html_block_by_tags`).
  The byte path's `parse_pandoc_blocks` reparse builds a fresh
  inner `RefsCtx` and re-disambiguates heading auto-ids — running
  it on a body whose headings ALREADY participate in the outer
  ctx's disambiguation produces `heading-1`/`subheading-1`
  instead of `heading`/`subheading`. Symptom: stray `-1` suffix
  on inner heading ids in pandoc-ast output.
- **Body-lifted signal is "no `HTML_BLOCK_CONTENT` child"**
  (covers both div and non-div). `div_has_structural_inner`
  (HTML_BLOCK_DIV) and `html_block_has_structural_lift`
  (HTML_BLOCK) both require: exactly two `HTML_BLOCK_TAG`
  children, both clean (open ends at `>`, close starts with
  `</`), no `HTML_BLOCK_CONTENT`. `HTML_BLOCK_CONTENT` persists
  only on still-opaque bodies (bq-wrapped, same-line non-div,
  open-trailing non-div, butted-close non-div, multi-line opens,
  matched-pair inline-block). Empty / blank-only bodies (no
  `HTML_BLOCK_CONTENT`, just optional `BLANK_LINE`s between tags)
  count as lifted.
- **`html_block_open_tag_is_clean` accepts "TEXT ends in `>`"**
  (2026-05-11), not "TEXT exactly equals `>`". Covers both div's
  split-`>` emission (`TEXT("<div") + ... + TEXT(">")`) and
  non-div's whole-line emission (`TEXT("<form>")`). Trailing
  content after `>` produces a TEXT NOT ending in `>` and
  correctly fails. The lift gate's
  `probe_clean_open_tag_line` rejects open-trailing at parse
  time, so the predicate's leniency only sees clean shapes.
- **Three Plain/Para demotion semantics via `LastParaDemote`
  enum** on `graft_document_children`:
  - `Never` — clean `<div>\nfoo\n</div>` and unbalanced bodies:
    trailing `Para` preserved.
  - `SkipTrailingBlanks` — `<div>` close-butted shape
    (`<div>foo</div>` / `<div>\nfoo</div>` / `<div>foo\n</div>`
    with content-then-`</div>`): demote LAST `PARAGRAPH` even
    past trailing `BLANK_LINE`s.
  - `OnlyIfLast` — non-div strict-block close: demote ONLY when
    the inner doc's last child is a `PARAGRAPH` (no trailing
    `BLANK_LINE`). Mirrors pandoc's top-level Para→Plain
    adjacency rule between a paragraph and the close-tag
    `RawBlock`.
- **Multi-line open tags emit multiple `HTML_ATTRS` regions** for
  `<div>`. `<div\n  id="x"\n  class="y">` produces one
  `HTML_ATTRS` per attribute line. Helpers that read via
  `.children().find(HTML_ATTRS)` see only the FIRST. Iterate and
  join with `" "` before parsing (`cst_div_open_tag_attr`).
- **All `<div>` shapes outside blockquotes lift** (clean
  multi-line, trailing-on-open, butted-close, indented-close,
  same-line, empty / blank-only). Same-line is gated on
  `probe_same_line_div_lift`; nested-close shapes fall back.
  **Non-div strict-block tags lift ONLY the clean multi-line
  shape** as of 2026-05-11 — same-line / open-trailing /
  butted-close fall through to opaque `HTML_BLOCK_CONTENT` and
  the projector byte walker.
- **Parser-side structural lift is gated on `bq_depth == 0`.**
  Inside blockquotes, content lines carry BLOCK_QUOTE_MARKER +
  WHITESPACE prefixes the parser keeps for losslessness. The
  projector handles bq context via
  `collect_html_block_text_skip_bq_markers` (see "Projector tag
  splitting"); parser-side lift inside bq is still deferred.

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` gets `PARAGRAPH` / `LIST` / etc. as direct children; `split_html_block_by_tags` / `flush_html_block_*` / `parse_pandoc_blocks` collapse into trivial CST walks; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All `<div>` shapes outside blockquotes lift structurally (2026-05-11)**: clean multi-line, trailing-on-open, butted-close, indented-close, same-line, empty / blank-only. **HTML blocks inside blockquotes project correctly via projector marker-strip (2026-05-11)**. **Fix #4 (non-div strict-block body lift) clean multi-line landed (2026-05-11)** for `<form>`, `<section>`, `<header>`, `<nav>`, `<aside>`, `<article>`, `<footer>`, `<p>`, `<table>`, `<tr>`, `<td>`, …. **Fix #4 shape extension landed (2026-05-11)** for non-div butted-close, open-trailing, and same-line. **Fix #4 multi-line open-tag lift landed (2026-05-11)** for non-div strict-block tags (`<form\n  id="x"\n  class="y">…`), with `HTML_ATTRS` exposure on each attribute line so salsa picks up multi-line `<section id>` declarations. Projector adds `open_tag_raw_block_text` to canonicalize multi-line open tags to pandoc's single-line `<tag attr1 attr2 ...>` form on RawBlock emission. All non-bq non-div strict-block shapes outside blockquotes (single-line + multi-line opens) now lift. Bq-wrapped non-div shapes still take the legacy `split_html_block_by_tags` byte-walker fallback. Matched-pair inline-block (`<video>`/`<iframe>`/`<button>`) still byte-walker. Pass count progression: 132 → 137 → 140 → 141 → 142 → 145 → 148 → 151 → 154 → 157 → 159 (2 new corpus cases). |

--------------------------------------------------------------------------------

## Latest session — 2026-05-11 (Phase 6 / Fix #4 multi-line open-tag lift for non-div strict-block)

Extended the same-day Fix #4 shape lift to the remaining
non-bq-wrapped non-div strict-block shape: multi-line open tags like
`<form\n  id="x"\n  class="y">…`. Parser now recognizes the
multi-line open via `find_multiline_open_end` and emits structural
`HTML_ATTRS` regions per attribute line via the generalized
`emit_multiline_open_tag_with_attrs(tag_name)` helper (renamed from
`emit_multiline_div_open_tag`). The body lift uses the existing
`emit_html_block_body_lifted` path — body lines between open and
close tags parse as fresh Pandoc markdown into structural CST
children. Salsa's existing `AttributeNode` descendants walk picks up
multi-line `<section id="intro">` ids as anchor declarations.

**Projector canonicalization**: multi-line open `HTML_BLOCK_TAG`
nodes carry literal source bytes (`<form\n  id="x"\n  class="y">`),
which would diverge from pandoc-native's `<form id="x" class="y">`
on RawBlock emission. `normalize_native` preserves whitespace inside
string literals, so the conformance harness sees the divergence.
New `open_tag_raw_block_text` helper in `pandoc_ast.rs` reconstructs
the open-tag text from structural tokens when `HTML_ATTRS` regions
are present, joining `<tagname` + ` ` + each attr (trimmed) + `>`.
Single-line opens without structural HTML_ATTRS keep their literal
text (already canonical).

**Pass count**: html 157 → 159, total 350 → 352.

### What landed

- Generalized `emit_multiline_div_open_tag` →
  `emit_multiline_open_tag_with_attrs(tag_name)` (parameterize
  `prefix_len = 1 + tag_name.len()`).
- Extended `multiline_open_end` detection in `parse_html_block` to
  Pandoc-lift-eligible strict-block tags via
  `is_pandoc_lift_eligible_strict_block_tag`.
- Removed `multiline_open_end.is_none()` requirement from
  `strict_block_tag_name`; added multi-line branch to
  `strict_block_lift` (`multiline_open_end.is_some()` ⇒ eligible —
  `find_multiline_open_end` already verified the close `>`).
- Open-tag dispatch: multi-line div + non-div lifts share
  `emit_multiline_open_tag_with_attrs`; void / other keep
  `emit_multiline_open_tag_simple`.
- New `open_tag_raw_block_text` projector helper canonicalizes
  the RawBlock text from structural tokens when `HTML_ATTRS` are
  present.

### Suggested next sub-targets

1. **Matched-pair inline-block lift `<video>`/`<iframe>`/`<button>`**
   (medium-large). The remaining big projector byte-walker user.
   Once lifted, `inline_pending` and
   `interior_starts_with_void_block_tag` can be pruned.
2. **Parser-side lift inside blockquotes** (medium). `> <div>...`,
   `> <form>...` still take projector marker-strip; parser-side
   lift would unify the path and let us delete
   `collect_html_block_text_skip_bq_markers`.
3. **Prune projector byte walkers** once 1–2 land —
   `split_html_block_by_tags` / `flush_html_block_*` /
   `parse_pandoc_blocks` would lose their last in-tree callers.

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs` — gate +
  helper generalization (~20 net ins).
- `crates/panache-parser/src/pandoc_ast.rs` — `open_tag_raw_block_text`
  helper, called from `emit_html_block_structural` (~50 ins).
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/` —
  cases 0351–0352 (form multi-line open, section multi-line open
  with id).
- `crates/panache-parser/tests/fixtures/cases/html_block_strict_block_multiline_open_{pandoc,commonmark}/`
  + snapshots + runner — paired parser fixture; pins CM/Pandoc
  divergence (CM keeps opaque `HTML_BLOCK_CONTENT`, Pandoc lifts).
- `tests/fixtures/cases/html_block_strict_block_multiline_open/` +
  runner — formatter golden (idempotent, unchanged formatting).
- Allowlist (ids 351–352 + section comment), report.txt + JSON.

### New trap

Folded into Persistent traps under "Projector tag splitting":
projector emission of lifted open `HTML_BLOCK_TAG` must canonicalize
to pandoc's single-line form when structural `HTML_ATTRS` are
present — literal multi-line bytes diverge under
`normalize_native`'s in-string whitespace preservation.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-11 — Phase 6 / Fix #4 non-div strict-block shape sweep (clean multi-line body, butted-close, open-trailing, same-line, empty `<div>`, bq-wrapped projector marker-strip, `<div>` shape lifts) — html 142 → 157 — `is_pandoc_lift_eligible_strict_block_tag`, `html_block_has_structural_lift`, `LastParaDemote::OnlyIfLast/SkipTrailingBlanks/Never`, `parse_with_refdefs` graft, `collect_html_block_text_skip_bq_markers`, generalized `emit_open_tag_tokens` + `probe_same_line_lift`.
- 2026-05-10 → 2026-05-11 — Phase 6 cannot_interrupt (`<style>`, PI, `</script>`, `<script type=math/tex>`) + Fix #1/#2 — html 132 → 142 — PARAGRAPH→PLAIN retag at YesCanInterrupt; `is_closing` field; `is_math_tex_script_open`; pandoc `isInlineTag` (issue #10643).
- 2026-05-10 — Strict-block/verbatim closing-form lift, multi-line void open-tag, incomplete-open recursion fix, Phase 3 void `eitherBlockOrInline` — html 105 → 132 — close-tag branches, `closes_at_open_tag`, `pandoc_html_open_tag_closes` gate, `PANDOC_VOID_BLOCK_TAGS`.
- 2026-05-09 — Phase 3 + Phase 5 (non-void eitherBlockOrInline; HTML5 sectioning; `<DIV>` losslessness; Plain/Para; multi-line attrs; refs inheritance) — html 62 → 105 — projector `inline_pending` + parser `cannot_interrupt`; CM/Pandoc blockHtmlTags split; `build_refs_ctx_inherited`.
- 2026-05-08 — Phases 1-5 seed (issue #263 closed) — html 0 → 62 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS` tokenization, sectioning/verbatim corpus pin, depth-aware nested `<div>`.
