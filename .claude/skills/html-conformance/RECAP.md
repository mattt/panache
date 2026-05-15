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
  tokens.** Expose attributes by tokenizing existing source bytes
  (split TEXT into `TEXT + WS + HTML_ATTRS{TEXT} + TEXT`).
  Synthetic tokens break the tree-text-equals-input invariant.
  Use source-byte slices (`&rest[..4]`), never literals (`"<div"`)
  for case-insensitive prefix matches.
- **Same-line `<div>foo</div>` is ONE `HTML_BLOCK_TAG`** — close
  lives inside a TEXT child of the open. Naive `strip_suffix('>')`
  grabs wrong `>`; scan to first **unquoted** `>`. Quoted attribute
  values hide `<` / `>`; tag-bracket scanners thread quote state
  across line boundaries (`count_tag_balance`,
  `find_multiline_open_end`, `pandoc_html_open_tag_closes`).
- **Multi-line open-tag close branches diverge by tag class** —
  void multi-line opens early-exit returning `end_line_idx + 1`
  BEFORE close-marker loop. `same_line_closed` short-circuit must
  guard `multiline_open_end.is_none()`.
- **Incomplete open tags (`<embed\n`, no `>` anywhere) caused
  projector infinite recursion.** Pandoc treats as paragraph text.
  Gate Pandoc BlockTag recognition on `pandoc_html_open_tag_closes`
  in `block_dispatcher::detect_prepared`. CommonMark stays liberal.
- **Self-closing `<tag/>` doesn't bump depth.** Depth-aware close
  matchers check `bytes[j-1] == b'/'` at closing `>`.
- **`input.lines()` strips newlines**; for losslessness-asserting
  parser tests use `split_lines_inclusive`.
- **`HtmlBlockType::BlockTag` is `Box<dyn Any>`-roundtripped via
  block dispatcher.** Adding a field works automatically; E0063
  points at every literal site.

### Pandoc tag categorization

- **Pandoc has THREE tag sets**: strict block (`PANDOC_BLOCK_TAGS`),
  inline-block non-void (`PANDOC_INLINE_BLOCK_TAGS`), inline-block
  void (`PANDOC_VOID_BLOCK_TAGS`). Strict always splits; non-void
  follows `inline_pending` + matched-pair lift; void follows
  `inline_pending` + emits single RawBlock. Source:
  `pandoc/.../TagCategories.hs` + `Readers/HTML.hs::isBlockTag` /
  `isInlineTag`. CommonMark and Pandoc `blockHtmlTags` lists differ
  in both directions (~15 tags); don't merge. Parser gates on
  `is_commonmark`; projector runs Pandoc only.
- **`eitherBlockOrInline` is context-dependent.** Mirror needs BOTH
  parser-side `cannot_interrupt` (don't break running paragraph) AND
  projector-side `inline_pending` (don't split mid-text).
- **Closing forms of all matched-pair tag sets ARE block starts
  under Pandoc** — each emits `BlockTag { closes_at_open_tag: true }`.
  Dispatcher's `cannot_interrupt` keys on inline-block + void only:
  strict-block + verbatim closes get `YesCanInterrupt`; inline-block
  / void closes stay inline in running paragraphs.
- **Verbatim tags fire before inline-block / strict-block arms** —
  `VERBATIM_TAGS` checked first; script-in-eitherBlockOrInline +
  style/textarea-in-blockHtmlTags overlap is harmless.
- **Pandoc `isInlineTag` special cases (issue #10643):** `<style>`
  open+close, `</script>`, PIs, comments, `<script
  type="math/tex…">` (case-insensitive, single-line) cannot
  interrupt paragraph. `<pre>` / non-math-tex `<script>` /
  `<textarea>` DO interrupt. Implemented in
  `HtmlBlockParser::detect_prepared`'s `cannot_interrupt`;
  requires `is_closing: bool` on `HtmlBlockType::BlockTag`.
- **Indented `isInlineTag` demotes to `Para [RawInline]`** under
  Pandoc — the same set as `cannot_interrupt` (Comment, PI,
  `<style>` o+c, `</script>`, math-tex `<script>`, Type7, inline-
  block matched-pair, void block tags). Parser-side gate in
  `HtmlBlockParser::detect_prepared` returns `None` when
  `leading_spaces(ctx.content) > list_indent_info.content_col`,
  so paragraph parsing picks up the line and emits `RawInline`.
  Trap: `ctx.content` retains list-item content_col indent
  (NOT auto-stripped). Blockquote markers ARE stripped from
  `ctx.content` — bq cases work transparently.
- **`HtmlBlockType::BlockTag.is_closing` — match guards pivoting on
  tag identity MUST check it.** `pandoc_html_open_tag_closes`
  returns true for both `<div>` and `</div>` (scans for first `>`).
  Gates firing on `tag_name == "div"` alone wrongly retag close
  forms. `HTML_BLOCK_DIV` retag destructures `is_closing: false`;
  `</div>` without matched open keeps opaque HTML_BLOCK → single
  RawBlock per pandoc-native.

### Projector tag splitting

- **`split_html_block_by_tags` walks bytes, not tokens.**
  Context-tracked via `inline_pending`; runs for opaque
  HTML_BLOCKs only (comments, PI, verbatim, void tags, unmatched
  strict / inline-block tags). Matched-pair `<div>` is parser-
  lifted now. `<video>...</video>` matched-pair lift abandons
  when interior opens with void block tag at col 0
  (`inline_block_void_interior_abandons`). Inline-block open with
  no matched close also emits RawBlock — falling through to
  `inline_pending=true` causes stack overflow via tail-text
  reparse recursion.
- **`inline_pending` resets on consecutive newlines (≥ 2).**
  Inter-tag text demotes Para→Plain when butted against next tag;
  tail text does NOT demote. Use `flush_html_block_text` vs
  `flush_html_block_tail_text`.
- **HTML blocks inside blockquotes need
  `collect_html_block_text_skip_bq_markers`** on remaining
  byte-walker paths — parser keeps `BLOCK_QUOTE_MARKER + WS` as
  structural tokens; passing `node.text()` re-recognizes `> ` as
  nested bq. Remaining caller: `emit_html_block` for verbatim in
  bq.
- **`walk_skip_bq_markers` also strips leading line-start
  `WHITESPACE`** when the token is NOT preceded by a
  `BLOCK_QUOTE_MARKER` on the same line. This covers the
  list-item indent re-injected by
  `strip_list_item_indent`/`LinePrefixState` (see "List-item
  HTML structural lift" section). The rule is unambiguous: the
  parser never emits a leading line-start `WHITESPACE` inside
  `HTML_BLOCK_CONTENT` or `HTML_BLOCK_TAG` outside the lift
  path — top-level indented HTML keeps the leading indent in a
  single `TEXT` token. The walker threads two flags
  (`skip_next_ws` for bq pairs, `at_line_start` for line-start
  WS) and flips `at_line_start` to `true` after each NEWLINE /
  BLANK_LINE token.
- **Projector `open_tag_raw_block_text` canonicalizes multi-line
  open tags.** With `HTML_ATTRS`, literal source diverges from
  pandoc's canonical single-line form (`normalize_native`
  preserves WS inside `"..."`). Helper walks
  `children_with_tokens`, takes leading `<tagname` TEXT, joins
  HTML_ATTRS trimmed texts with single spaces, appends `>`.
  Single-line opens without HTML_ATTRS keep literal text.

### Refs / footnotes / heading-id resolution

- **`parse_pandoc_blocks` swaps in an inner `RefsCtx`** for
  recursive reparse. Swap belongs IN `parse_pandoc_blocks`, not
  at call sites. `build_refs_ctx` mutates `REFS_CTX` mid-build —
  when swapping save outer FIRST via `mem::take`, THEN call
  `build_refs_ctx`, THEN install.
- **`heading_id_by_offset` is offset-keyed, not slug-keyed.**
  Inner CST's offsets are zero-based; don't copy outer
  `heading_ids` into inner. Build fresh inner ctx and inherit
  cross-boundary refs/footnotes via `build_refs_ctx_inherited`.
- **`fenced_div` walks structural CST via `collect_block`** —
  doesn't use `parse_pandoc_blocks`. Don't generalize the swap
  to fenced divs.
- **`AttributeNode::can_cast` accepts `HTML_ATTRS`**; the salsa
  walk picks up `<div id>` / `<span id>` and non-div strict-block
  tag ids (`<section id="x">`, etc.) automatically. Diverges
  from pandoc-native (which keeps them as RawBlock without
  lifting attrs) but matches user intent for anchor-link
  resolution. No parallel salsa walk.

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
  adjacency** is parser-side
  (`Parser::close_paragraph_as_plain_if_open` +
  `html_block_demotes_paragraph_to_plain`, wired at
  YesCanInterrupt in `core.rs`). CST emits `PLAIN`; projector
  trivially maps. Don't reintroduce projector-side demotion.
- **Formatter non-idempotency for tab-indented list items.**
  `-\t<div>\n\thello\n\t</div>` parses correctly as
  `Div [Para [Str "hello"]]` but the formatter normalizes
  `-\t` to `- ` while keeping body tabs — round-trip then
  re-parses as `Div [CodeBlock "hello"]` (tab exceeds new
  content_col 2). Formatter bug, not html-conformance.
  Parser fixtures + conformance pin parser side only; no
  formatter goldens for tab-indented list-item HTML shapes.
  Fix likely in `formatter/lists.rs`.

### Latent projector panic on unstructural HTML_BLOCK_DIV

`pandoc_ast.rs::html_div_block` `debug_assert!`s on
`HTML_BLOCK_DIV` lacking structural inner shape ("parser
regression"). Any future parser change that retags
`HTML_BLOCK_DIV` MUST guarantee structural lift; otherwise top-
level `<div>foo</div>bar` (or any new shape that retags but skips
the lift gate) will panic at projection time. Prefer "fall back to
opaque HTML_BLOCK" over silently emitting a one-child
HTML_BLOCK_DIV. Same-line lift gate (`probe_same_line_lift`) is
contains-based as of 2026-05-15 (was: ends_with) — `count_tag_balance
== (0, 1)` is now load-bearing for rejecting multi-close shapes.

### Projector-as-second-stage-parser smell (architectural)

`pandoc_ast.rs` is the public `to_pandoc_ast` API; linter / salsa
/ LSP / formatter walk the CST, not the projector. Phases 1/5
landed structural retags (`HTML_BLOCK_DIV`, `INLINE_HTML_SPAN`);
Phase 6 lifted inner content of `<div>` / non-div strict-block /
inline-block matched-pair shapes (non-bq + bq) into CST children.
Vestigial `<div>` byte walkers (`try_div_html_block`, etc.)
pruned 2026-05-11. Load-bearing remainder: `split_html_block_by_tags`
(opaque HTML_BLOCKs only), `parse_pandoc_blocks` (inter-tag text
reparse via `flush_html_block_text` /
`flush_html_block_tail_text`), `collect_html_block_text_skip_bq_markers`
(one `<pre>` verbatim-in-bq case + multi-line-open-in-bq
fallback), table-cell reparses. `html_div_block` `debug_assert!`s
on unlifted HTML_BLOCK_DIV.

### Structural lift (Fix #3 / Fix #4 family)

- **Recursive parse uses `parse_with_refdefs`, not `parse`.**
  `parse` re-runs `populate_refdef_labels` on JUST the inner
  text, hiding outer refdefs from inner reference links. Thread
  outer config's `refdef_labels` through.
- **Line-consumption boundary trap** (Comment / PI trailing split,
  2026-05-13). `parse_html_block_with_wrapper`'s `lines: &[&str]`
  is the WHOLE document, not just the current container's
  content. Returning `lines.len()` from inside a fenced div /
  list item / blockquote consumes container close markers
  (`:::`, `> `, list-marker indent). Sibling-emit helpers
  (`graft_document_children` after `builder.finish_node()`)
  should only consume the current line; the outer dispatcher
  resumes at `close_line + 1` to keep container boundaries
  intact. Trade-off: multi-line softbreak continuation
  (`<!-- --> A\nB` → `Para [A, SoftBreak, B]`) breaks because
  the outer dispatcher starts a fresh paragraph for `B` —
  blocked.txt entry 0390 tracks the gap.
- **`graft_document_children` works as a sibling-emit helper**,
  not just an inside-HTML_BLOCK helper. Call it AFTER
  `builder.finish_node()` on HTML_BLOCK and it grafts children
  at the parent (DOCUMENT / container) level — that is what
  the Comment / PI trailing-split uses.
- **`HTML_BLOCK_DIV` retag at dispatcher is two-pronged.** Retag
  fires iff `probe_open_tag_line_has_close_gt(ctx.content, "div")`
  (single-line) OR `pandoc_html_open_tag_closes(lines, line_pos,
  bq_depth)` (multi-line). Incomplete opens (`<div\n` no `>`
  anywhere) keep opaque HTML_BLOCK so projector treats as
  paragraph text. Multi-line + trailing on close-`>` line:
  `emit_multiline_open_tag_with_attrs` captures trailing into
  `pre_content` via `lift_trailing=true` so open `HTML_BLOCK_TAG`
  ends cleanly with `TEXT(">")`.
- **Lifted HTML_BLOCK / HTML_BLOCK_DIV MUST route structural,
  not byte path.** `collect_block` routes `HTML_BLOCK_DIV` →
  `html_div_block`; `emit_html_block` routes lifted HTML_BLOCK →
  `emit_html_block_structural` (not `split_html_block_by_tags`).
  Byte path's `parse_pandoc_blocks` builds fresh inner `RefsCtx`
  → re-disambiguates heading auto-ids, producing stray `-1`
  suffix. Body-lifted signal: no `HTML_BLOCK_CONTENT` child;
  `html_block_open_tag_is_clean` accepts TEXT ending in `>`.
- **`LastParaDemote` enum** on `graft_document_children`:
  `Never` (clean/unbalanced — Para preserved), `SkipTrailingBlanks`
  (div close-butted — demote LAST PARAGRAPH past trailing
  BLANK_LINEs), `OnlyIfLast` (non-div strict-block close —
  demote only when last child is PARAGRAPH with no trailing
  BLANK_LINE).
- **Multi-line open tags emit multiple `HTML_ATTRS` regions** —
  one per attribute line. Iterate + join with `" "` (see
  `cst_div_open_tag_attr`); `.children().find()` only sees first.
- **All non-bq shapes lift** for `<div>` and non-div Pandoc
  strict-block + inline-block matched-pair tags: clean
  multi-line, open-trailing, butted-close, indented-close,
  same-line, empty/blank-only, multi-line open + trailing.
- **Bq lift covers clean + same-line + messy + multi-line-open-
  clean.** Open-line `> ` consumed by outer BLOCK_QUOTE;
  subsequent lines' `> ` re-injected via `BqPrefixState`. Deeper
  bq (`> > <div>`) works transparently. `find_multiline_open_end`
  + `emit_multiline_open_tag_with_attrs/_simple` thread `bq_depth`
  and re-emit `BLOCK_QUOTE_MARKER + WHITESPACE` prefix tokens for
  lines past `start_pos` (line 0's prefix is owned by outer BQ).
- **Bq prefix re-injection: both `NEWLINE` *and* `BLANK_LINE`
  token (kind, not node) advance `line_idx`.** Inner parse puts
  `BLANK_LINE` token (text `"\n"`) inside `BLANK_LINE` node;
  treating only NEWLINE mis-aligns prefixes — losslessness
  violation when blank line precedes content line in body.
- **Three bq lift gates by `depth` after open line.** All require
  `bq_depth > 0` + `depth_aware_tag.is_some()` + tag in
  `is_pandoc_lift_eligible_block_tag`. Inline-block matched-pair
  also gates on NOT `inline_block_void_interior_abandons`.
  Discriminators:
  - `same_line_bq_lift_tag` — `depth <= 0`, single-line. Routes
    through `same_line_closed` branch; uses
    `emit_html_block_body_lifted` with `bq: &mut None`.
    Demote: div=SkipTrailingBlanks, non-div=OnlyIfLast.
  - `bq_clean_lift` — `depth > 0` + close line is `trim_start
    .starts_with("</")` + clean open (`pre_content.is_empty()`).
    Accepts single + multi-line opens. Calls
    `emit_html_block_body_lifted_bq`. Demote: div=Never (Para
    preserved), non-div=OnlyIfLast.
  - `bq_messy_lift_tag` — `depth > 0` + NOT clean. Accepts both
    open shapes; multi-line + trailing uses `lift_trailing` so
    trailing → `pre_content`. Close-marker site bq-strips then
    `try_split_close_line`. Calls
    `emit_html_block_body_lifted_bq_messy`. Demote: div keyed on
    close-butted (Never when `leading` empty, else
    SkipTrailingBlanks); non-div=OnlyIfLast.
- **`try_split_close_line` whitespace-only `leading` is close-tag
  indent, not body content.** For `   </article>`, classify
  whitespace-only via `leading.bytes().all(|b| b == b' ' || b ==
  b'\t')`, pass `body_leading=""` to recursive parse, emit
  leading bytes as `WHITESPACE` inside close `HTML_BLOCK_TAG`.
  Keep demote policy keyed on **original** `leading.is_empty()`.
- **Bq messy-lift duplicate-prefix trap.**
  `emit_html_block_body_lifted_bq_messy` injects close line's bq
  prefix in front of `leading` via BqPrefixState; close
  `HTML_BLOCK_TAG` MUST NOT re-emit `emit_bq_prefix_tokens`
  when `leading` is non-empty (doubles `> ` bytes).
- **Projector `open_tag_raw_block_text` strips bq markers AND
  leading 1-3 space indent.** Bq-wrapped close `> </form>`
  carries `BLOCK_QUOTE_MARKER + WHITESPACE` leading tokens;
  open-line `  <article>` carries standalone `WHITESPACE` before
  tag-name TEXT. Pandoc-native `RawBlock` text is tag bytes only
  — helper skips bq prefix pairs AND leading `WHITESPACE` before
  the accumulator collects its first non-WS token. HTML_ATTRS
  branch (multi-line open canonicalization) unaffected.

### List-item HTML structural lift

- **`ListItemBuffer::emit_as_block` lifts same-line / fully-
  contained HTML blocks via `try_emit_html_block_lift`.** Gate is
  strict: `try_parse_html_block_start` must recognize the first
  line, the inner reparse must produce exactly ONE top-level child
  of kind `HTML_BLOCK` / `HTML_BLOCK_DIV`, the child must consume
  every byte of the buffer text, and `HTML_BLOCK_DIV` requires
  ≥ 2 `HTML_BLOCK_TAG` children (matched open+close). Multi-line
  shapes (`- <section>\n  hello\n  </section>`, `- <video>\n  body\n
  </video>`) also lift as of 2026-05-13 — see "Close-form
  dispatcher gate" trap.
- **Close-form dispatcher gate (multi-line list-item HTML).** The
  dispatcher's HTML-block close-form recognition (`</div>`,
  `</section>`, `</pre>`, …) is gated on the enclosing LIST_ITEM
  buffer NOT having an unclosed matched-pair open of the same
  tag. Mechanism: `BlockContext::list_item_unclosed_html_block_tag:
  Option<String>` is populated in `parse_line` via
  `Parser::list_item_unclosed_html_block_tag` → `ListItemBuffer::
  unclosed_pandoc_matched_pair_tag` → which inspects the first
  buffer text segment with `try_parse_html_block_start`, checks
  it's a `BlockTag { is_closing: false }` matching
  `is_pandoc_matched_pair_tag`, then walks all buffer text
  segments calling `count_tag_balance`. When opens > closes,
  returns the tag name; `HtmlBlockParser::detect_prepared`
  returns `None` for close-forms whose tag matches the field.
  The buffer then accumulates the full matched-pair text, and
  `try_emit_html_block_lift` reparses + grafts. `count_tag_balance`,
  `is_pandoc_lift_eligible_block_tag`, and new
  `is_pandoc_matched_pair_tag` are now `pub(crate)`. The gate
  only fires under Pandoc dialect.
- **List-item indent normalization via `strip_list_item_indent`
  + `LinePrefixState` re-injection.** `emit_as_block` threads
  `Container::ListItem::content_col` to
  `try_emit_html_block_lift`. When `> 0`,
  `strip_list_item_indent` strips up to `content_col`
  leading-space bytes from each line after line 0 (line 0's
  leading is owned by the list marker), returns per-line
  prefix vector. Inner reparse runs on stripped text; graft
  re-injects each prefix as a `WHITESPACE` token at line start
  via `LinePrefixState` (mirrors `BqPrefixState`). Without
  this, `- <div>\n  body\n  </div>` triggers indented-close
  demote (Plain not Para) and `<pre>` keeps indent in RawBlock.
  Tab handling: advance col by 4 on `\t`, refuse to split a
  tab that would overshoot. Injected WHITESPACE inside opaque
  `HTML_BLOCK_CONTENT` / `HTML_BLOCK_TAG` is stripped by
  projector's `walk_skip_bq_markers` line-start rule; inside
  lifted PARAGRAPH/PLAIN it becomes leading `Inline::Space`
  and `coalesce_inlines` edge-trim drops it.
- **`format_list_item` silently drops `LIST_MARKER` when the
  list item has NO `PLAIN`/`PARAGRAPH` content_node.** The
  marker-emit pass is wired to the wrapping flow which produces
  no output without a content_node. Per-kind arms in the
  nested-blocks loop emit the marker when
  `no_content_emitted && is_first_real_child`: existing
  `HORIZONTAL_RULE` arm, added `HTML_BLOCK | HTML_BLOCK_DIV` arm
  for the same-line HTML lift. Any new structural lift that
  produces a list-item-as-block CST shape (HEADING-only,
  BLOCK_QUOTE-only, FENCED_DIV-only, etc.) MUST update
  `format_list_item` with the same pattern or the marker
  silently disappears. The `_` fallback at the end of the loop
  just calls `format_node_sync` with content_node — it does
  NOT emit the marker.
- **`find_content_node` skips PLAIN/PARAGRAPH trailing a leading
  `HTML_BLOCK`/`HTML_BLOCK_DIV`.** Without the guard, the
  formatter picks the trailing PLAIN (from the comment/PI
  trailing-text-split list-item shape `- <!-- hi --> trailing`)
  as the wrap source, emits `- trailing` on the marker line,
  then drops the HTML_BLOCK below — producing the broken
  non-idempotent `- trailing\n<!-- hi -->`. With the guard the
  function returns None for this shape; the HTML_BLOCK arm
  handles the marker line and the trailing PLAIN runs through
  the continuation-paragraph path, yielding the idempotent
  `- <!-- hi -->\n  trailing`. The guard also returns None for
  any non-PLAIN/PARAGRAPH/BLANK_LINE first-real child after the
  marker (the wrap source must be the FIRST PLAIN/PARAGRAPH, or
  there's no wrap source).

--------------------------------------------------------------------------------

## Phase progress

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | `<div>` block lift (HTML_BLOCK_DIV + HTML_ATTRS structural) | **Wrapper retag landed** (2026-05-08) — issue #263 closed; `<DIV>` losslessness fix landed. **Inner content NOT yet lifted into CST children** — still raw `HTML_BLOCK_CONTENT` TEXT tokens; projector reparses them. |
| 2 | `<span>` inline lift (INLINE_HTML_SPAN) | **Wrapper retag landed** (2026-05-08). Inner inlines mostly trivial (no recursive reparse needed). |
| 3 | Sectioning + verbatim corpus pin; `eitherBlockOrInline` lift | **Conformance landed** — non-void (2026-05-09); void (`<embed>`/`<area>`/`<source>`/`<track>`) (2026-05-10). Implementation leans on projector-side `inline_pending` tracking + byte walker; CST still opaque for split/matched-pair shapes. |
| 4 | Comments, PIs, declarations, CDATA projection | **Conformance landed** (2026-05-08); type-4 CM lowercase still gappy. CST opaque (these constructs project as RawBlock / RawInline). |
| 5 | `markdown_in_html_blocks` interaction edge cases | **Conformance landed** — depth-aware nested div, Plain/Para promotion, refs inheritance, **projector-level splitter** (`split_html_block_by_tags` byte walker + `parse_pandoc_blocks` recursive reparse), outer-matched-pair-abandons-on-void-interior. **The structural CST lift was deferred** — Phase 5's mechanism is the projector reparsing bytes, not the parser emitting structure. |
| 6 (new) | Lift inner HTML block content into structural CST children — `HTML_BLOCK_DIV` / `HTML_BLOCK` get `PARAGRAPH` / `LIST` / etc. as direct children; projector byte walkers become vestigial; `PARAGRAPH→PLAIN` retag at adjacent-HTML-block boundary. | **All non-bq + bq shapes lifted for `<div>` and non-div Pandoc strict-block tags as of 2026-05-12.** Shapes covered: clean multi-line, open-trailing, butted-close, indented-close, same-line, same-line + trailing-text-after-close (2026-05-15), empty / blank-only, multi-line open (clean and trailing). Inline-block matched-pair abandons when body begins with a void block tag (Plain via OnlyIfLast). Bq via three discriminator gates (`bq_clean_lift`, `same_line_bq_lift_tag`, `bq_messy_lift_tag`) — see "Three bq lift gates" trap. Dispatcher's `HTML_BLOCK_DIV` retag gate uses `pandoc_html_open_tag_closes` AND requires `is_closing: false`. Vestigial `<div>` byte walkers pruned 2026-05-11. **As of 2026-05-12** same-line / fully-contained HTML blocks lift inside list items (`ListItemBuffer::emit_as_block` reparse + graft path); formatter's `format_list_item` gets a `HTML_BLOCK / HTML_BLOCK_DIV` arm to emit the marker for these. **As of 2026-05-13** multi-line HTML blocks lift inside list items for non-div strict-block + inline-block + verbatim matched-pair tags via a close-form dispatcher gate (`BlockContext::list_item_unclosed_html_block_tag` + `ListItemBuffer::unclosed_pandoc_matched_pair_tag`); **same day**, list-item indent normalization (`strip_list_item_indent` + `LinePrefixState`) closes the `<div>` Plain→Para gap and the verbatim-tag (`<pre>`, `<style>`, `<script>`, `<textarea>`) RawBlock-indent gap. Projector's `walk_skip_bq_markers` strips leading line-start `WHITESPACE` to make the parser-side re-injection invisible to opaque-HTML projection. List-item Comment/PI with trailing text on the same item (`- <!-- hi --> trailing`, `- <?php foo ?> trailing`, newline-between, loose variant) lifts via a 2-child branch in `try_emit_html_block_lift` that accepts the existing trailing-split helper's HTML_BLOCK + PARAGRAPH output and retags the trailing PARAGRAPH to PLAIN for tight items. Formatter's `find_content_node` gained a guard so a PLAIN trailing an HTML_BLOCK isn't picked as the wrap source. **As of 2026-05-15 (latest)** same-line `<div>foo</div>bar` / `<form>foo</form>bar` (and bq, list-item, with-id variants) lift to `Div + Para[bar]` / `RawBlock + Plain + RawBlock + Para[bar]`. Parser same-line lift gate (`probe_same_line_lift`) accepts trailing text after `</tag>` (was: ends_with → contains, count_tag_balance still required `(0, 1)`); same-line-lift body splits the `try_split_close_line` close_part via new `split_close_marker_end` into close-marker bytes (`</tag>`) and trailing bytes, finishes the wrapper, then recursively parses + grafts trailing as siblings. List-item buffer's `try_emit_html_block_lift` 2-child branch widened to accept `HTML_BLOCK_DIV + PARAGRAPH`. Closes a latent projector panic on top-level `<div>foo</div>bar` (`debug_assert!`("HTML_BLOCK_DIV without structural inner shape")). **Pass count history: 105 → 235** (current). Open shape gaps tracked in latest session's "Suggested next sub-targets". |

--------------------------------------------------------------------------------

## Latest session — 2026-05-15 (Same-line div/strict-block trailing-text split)

Took sub-target #3 from the previous session (list-item div-with-
trailing) and the negative-space pinning sub-targets #4/#5 in one
sweep. The fix is universal — same-line trailing text after a
matched `</tag>` now lifts at top-level, in blockquotes, and
inside list items. Closed a latent projector panic
(`debug_assert!("HTML_BLOCK_DIV without structural inner shape —
parser regression")`) that had been masking `<div>foo</div>bar` at
top level. Net +9 cases (6 affirmative lifts + 3 negative-space
pins).

Conformance: html 226 → 235 (+9), total 420 → 429 (+9); 1 case
(0390 softbreak continuation) still blocked.

### What landed

- `probe_same_line_lift` (in
  `crates/panache-parser/src/parser/blocks/html_blocks.rs`) now
  checks the close marker is *contained* in the post-`>` slice
  (was: `ends_with`), still requires `count_tag_balance == (0, 1)`
  (no nested same-tag opens, exactly one close). This widens the
  gate to accept trailing text without inviting malformed shapes.
- Same-line-lift body in `parse_html_block_with_wrapper` splits
  the `try_split_close_line` close_part via new
  `split_close_marker_end(close_part, tag_name)` into
  close-marker bytes (`</tag>`) and trailing bytes. Empty
  trailing → existing single-line emit. Non-empty trailing →
  emit close `HTML_BLOCK_TAG` with TEXT(`</tag>`) only, finish
  wrapper, then `parse_with_refdefs(trailing + post_nl)` and
  graft via `graft_document_children(LastParaDemote::Never)`.
  Mirrors the existing Comment/PI trailing-split.
- `split_close_marker_end` is quote-aware (single+double quotes)
  so closing `>` inside attribute values doesn't trip it.
- List-item buffer's `try_emit_html_block_lift` 2-child branch
  widened to also accept `HTML_BLOCK_DIV + PARAGRAPH` (was:
  HTML_BLOCK + PARAGRAPH only). The `html_block_tag_count >= 2`
  guard moved out of the single-child gate and applies to both
  paths — div lifts always require matched open + close.
- 2 paired parser fixtures (`html_block_div_same_line_trailing`
  and `list_item_html_div_same_line_trailing`, each with
  CommonMark+Pandoc variants).
- 2 formatter goldens
  (`html_block_div_same_line_trailing`,
  `list_item_html_div_same_line_trailing`) pinning idempotent
  output `<div>foo</div>\nbar` / `- <div>foo</div>\n  bar`.
- 9 new corpus cases (0421-0429):
  - 0421-0424: `<div>foo</div>bar` top-level / with-id / bq /
    list-item.
  - 0425-0426: `<form>foo</form>bar` top-level / bq.
  - 0427-0429: negative-space pins for `>   <!-- hi --> trailing`,
    `> > <!-- hi -->`, `>>   <!-- hi -->` (already passing —
    pinned against regression).

### Files in committable diff

- `crates/panache-parser/src/parser/blocks/html_blocks.rs`.
- `crates/panache-parser/src/parser/utils/list_item_buffer.rs`.
- 2 paired parser fixtures + `golden_parser_cases.rs` registry
  edits + 4 CST snapshots.
- `crates/panache-parser/tests/fixtures/pandoc-conformance/corpus/0421…0429/`.
- `crates/panache-parser/tests/pandoc/{allowlist.txt,report.txt}`
  + `docs/development/pandoc-report.json`.
- 2 formatter goldens + `tests/golden_cases.rs` registry edit.

### Deferred / probed gaps

- **Bq-in-listitem stacked container** (`- > <div>\n  > hello\n  >
  </div>`) still broken: `<div>` is parsed as INLINE_HTML inside
  the bq's PLAIN, not as HTML_BLOCK_DIV. The bq dispatcher inside
  a list item doesn't fire the matched-pair gate for the multi-
  line `<div>...</div>` shape. Needs work in the bq dispatcher
  inside list items — likely a bigger fix touching the multi-line
  open recognition's interaction with list-item content_col.
- **Softbreak continuation across HTML-block boundary** (0390
  still blocked).
- **Top-level `<div>foo</div></div>` projects the second `</div>`
  as RawBlock per pandoc, which already works after the fix
  (recursive parse of `</div>\n` falls to the unmatched-close
  HTML_BLOCK path). No corpus pin yet — could add one.

### Suggested next sub-targets

1. **Bq-in-listitem stacked container** (carry-over from prior
   sessions). Likely the largest remaining piece.
2. **Softbreak continuation** for 0390. Requires fusion of
   adjacent Para siblings.
3. **`<div>foo</div></div>` top-level pin** (corpus expansion to
   pin the unmatched-trailing-close behavior — already works).
4. Probe `<div>foo<inner>x</inner></div>bar` (div with non-div
   nested inner) and `<div><div>x</div></div>bar` (nested same-tag
   trailing) — confirmed pandoc shape; check whether the lift
   handles them (likely yes given count_tag_balance is tag-name
   specific) and add corpus pins.
5. **Multi-line + trailing-after-close** for `<div>\nfoo\n</div>bar`
   — currently the existing butted-close lift path (via
   `try_split_close_line` on the close line in the multi-line
   loop) is unmodified; check whether trailing text on the close
   line lifts symmetrically and add corpus.

### New traps

- **Latent projector panic when `HTML_BLOCK_DIV` lacks structural
  inner shape**: `pandoc_ast.rs::html_div_block` `debug_assert!`s
  on this. Any future parser change that retags `HTML_BLOCK_DIV`
  must guarantee structural lift; otherwise top-level
  `<div>foo</div>bar` (or any other shape that retags but skips
  the lift gate) will panic. Prefer "fall back to opaque
  HTML_BLOCK" over silently emitting a one-child HTML_BLOCK_DIV.
- **`probe_same_line_lift` is now contains-based, not ends-with**:
  any new same-line shape needs to verify `count_tag_balance ==
  (0, 1)` is sufficient — multiple closes silently fall through
  to the byte path now (caught by the `(0, 1)` guard), but new
  callers must not loosen it further.

--------------------------------------------------------------------------------

## Earlier sessions (compact log)

Newest first. One line per session: date — phase/sub-target — pass
count delta — root cause / lever.

- 2026-05-13 — Phase 6 — list-item Comment/PI trailing-text split: `- <!-- … --> trailing` and variants lift via 2-child branch in `try_emit_html_block_lift` (HTML_BLOCK + PARAGRAPH); formatter `find_content_node` PLAIN-after-HTML_BLOCK guard — html 222 → 226 — widen `try_emit_html_block_lift`; `graft_node_retag_root` for Para→Plain.
- 2026-05-13 — Phase 6 — indented `isInlineTag` demotion: pandoc demotes every `cannot_interrupt` tag (Comment, PI, `<style>` o+c, `</script>`, math-tex `<script>`, Type7, inline-block matched-pair, void) when `leading_spaces(ctx.content) > list_indent_info.content_col` — html 214 → 222 — parser-side gate in `HtmlBlockParser::detect_prepared`; `<pre>` / `<script>` / `<textarea>` stay RawBlock with the leading-indent strip.
- 2026-05-13 — Phase 4/6 — Comment/PI trailing-text split wave: top-level `<!--…--> trailing` / `<?php …?> trailing` → `RawBlock + Para [trailing]` via new `try_parse_comment_pi_with_trailing_split`; extended to bq via `emit_bq_prefix_tokens` and trailing-WS trim; `emit_html_block` strips first-line 1-3 spaces of leading indent; `<span>` corpus pins (autolink/image alt/heading/link-text/emph/setext); 5+6 corpus pins (0375-0385) for list-item div/span shapes and inline-span variants — html 187 → 214 — narrowly gated parser-side helper + projector RawBlock trim; 0390 softbreak continuation blocked.
- 2026-05-13 — Phase 6 — list-item indent normalization (`strip_list_item_indent` + `LinePrefixState` re-injection; projector `walk_skip_bq_markers` line-start-WS strip) closes `<div>` Plain→Para gap and verbatim-tag (`<pre>`, `<style>`, `<script>`, `<textarea>`) RawBlock-indent gap — html 176 → 181 — `ListItemBuffer::emit_as_block` threads `content_col` from `Container::ListItem`; per-line `WHITESPACE` re-injection during graft mirrors `BqPrefixState`.
- 2026-05-13 — Phase 6 — multi-line list-item HTML lift via close-form dispatcher gate (`- <section>...`, `- <video>...`, `- <iframe>...`, `- <span>...`) — html 171 → 176 — `BlockContext::list_item_unclosed_html_block_tag` + `ListItemBuffer::unclosed_pandoc_matched_pair_tag`; `count_tag_balance` / `is_pandoc_lift_eligible_block_tag` / new `is_pandoc_matched_pair_tag` promoted to `pub(crate)`; close-form `</tag>` dispatch suppressed when the enclosing LIST_ITEM has an unclosed matched-pair open. Indent gap for `<div>` body and verbatim content deferred to next session.
- 2026-05-11 → 2026-05-12 — Phase 6 — non-div strict-block + bq + list-item structural lift wave — html 142 → 171 — `is_pandoc_lift_eligible_block_tag`, `html_block_has_structural_lift`, `LastParaDemote::{OnlyIfLast,SkipTrailingBlanks,Never}`, `parse_with_refdefs` graft, `emit_multiline_open_tag_with_attrs`; three bq discriminator gates + `BqPrefixState`; `ListItemBuffer::try_emit_html_block_lift` + formatter LIST_MARKER arm; `<div>` byte-walker prune; `open_tag_raw_block_text` leading-WS strip; dispatcher `is_closing: false` retag gate. Pruned vestigial `try_div_html_block`.
- 2026-05-08 → 2026-05-11 — Phases 1-5 seed + Phase 3 void `eitherBlockOrInline` + cannot_interrupt + Fix #1/#2 — html 0 → 142 — `HTML_BLOCK_DIV`/`INLINE_HTML_SPAN` retag, `HTML_ATTRS`, projector `inline_pending`, CM/Pandoc blockHtmlTags split, `closes_at_open_tag`, `pandoc_html_open_tag_closes`, `PANDOC_VOID_BLOCK_TAGS`, PARAGRAPH→PLAIN retag at YesCanInterrupt, `is_closing` field, `is_math_tex_script_open`, pandoc `isInlineTag` (issue #10643).
