//! Top-level YAML document orchestration.
//!
//! Walks `YAML_STREAM` → `YAML_DOCUMENT` → body containers and
//! dispatches to per-container renderers
//! ([`block_map`](super::block_map),
//! [`block_sequence`](super::block_sequence),
//! [`flow`](super::flow), [`scalar`](super::scalar)).
//!
//! Phase 1.8 status: five rules are applied across the render pipeline.
//! The token walk that builds `raw` already applies rule 8 (collapse
//! whitespace before an inline `YAML_COMMENT` to exactly one space) so
//! that the only CST-offset-dependent pass — rule 1's depth lookup —
//! runs against precomputed per-CST-line depths (decoupled from the
//! mutable buffer). After that, rule 1 (canonical 2-space indent per
//! `YAML_BLOCK_MAP_ENTRY`/`YAML_BLOCK_SEQUENCE_ITEM` nesting depth),
//! rule 10 (strip trailing whitespace per line), rule 7 (collapse runs
//! of multiple blank lines to one), and rule 13 (exactly one `\n` at
//! EOF) run as line-level post-passes in that order. Block scalar
//! (`|`/`>`) interior lines are skipped by rule 1 — their indent is
//! baked into a single `YAML_SCALAR` token and full canonicalization
//! needs a real block-scalar renderer (deferred). Token bodies inside
//! each line are otherwise emitted verbatim — per-container restyling
//! (quote style, flow spacing / wrap, …) has not landed yet.

use panache_parser::SyntaxNode;
use panache_parser::syntax::{SyntaxKind, SyntaxToken};
use rowan::{TextSize, TokenAtOffset, WalkEvent};

use super::options::YamlFormatOptions;

/// Render the given CST root into a string. The root is expected to be
/// the `DOCUMENT` node returned by
/// [`panache_parser::parser::yaml::parse_yaml_tree`], but any CST node
/// works for the token walk — we just iterate tokens.
pub(super) fn render(root: &SyntaxNode, _opts: &YamlFormatOptions) -> String {
    let depths = precompute_line_depths(root);
    let raw = walk_with_inline_comment_normalization(root);
    let indented = apply_canonical_indents(&raw, &depths);
    let stripped = strip_trailing_whitespace_per_line(indented);
    let collapsed = collapse_blank_line_runs(stripped);
    normalize_trailing_newline(collapsed)
}

/// STYLE.md rule 8: walk tokens left-to-right and, when emitting a
/// `WHITESPACE` token immediately preceding an inline `YAML_COMMENT`,
/// emit exactly one space instead of the original bytes. An "inline"
/// comment is one with non-whitespace content earlier on the same line
/// (i.e. the previous non-WHITESPACE token is not `NEWLINE`). Comments
/// at line start (standalone, preceded by `NEWLINE` or at file start)
/// keep their original surrounding whitespace.
///
/// All other tokens emit verbatim. Inline-comment normalization is the
/// only mutation here; it has to happen during the token walk because
/// later line-level passes don't have CST kind information.
fn walk_with_inline_comment_normalization(root: &SyntaxNode) -> String {
    let tokens: Vec<SyntaxToken> = root
        .preorder_with_tokens()
        .filter_map(|ev| match ev {
            WalkEvent::Enter(rowan::NodeOrToken::Token(t)) => Some(t),
            _ => None,
        })
        .collect();
    let mut raw = String::with_capacity(root.text_range().len().into());
    for (i, tok) in tokens.iter().enumerate() {
        if tok.kind() == SyntaxKind::WHITESPACE && is_ws_before_inline_comment(&tokens, i) {
            raw.push(' ');
        } else {
            raw.push_str(tok.text());
        }
    }
    raw
}

/// True if `tokens[i]` is a `WHITESPACE` token whose run of contiguous
/// whitespace ends with a `YAML_COMMENT`, AND that comment is inline
/// (has any non-whitespace token earlier on the same line).
fn is_ws_before_inline_comment(tokens: &[SyntaxToken], i: usize) -> bool {
    let mut j = i + 1;
    while j < tokens.len() && tokens[j].kind() == SyntaxKind::WHITESPACE {
        j += 1;
    }
    if j >= tokens.len() || tokens[j].kind() != SyntaxKind::YAML_COMMENT {
        return false;
    }
    let mut k = i;
    while k > 0 {
        k -= 1;
        match tokens[k].kind() {
            SyntaxKind::NEWLINE => return false,
            SyntaxKind::WHITESPACE => continue,
            _ => return true,
        }
    }
    false
}

/// Precompute the canonical indent depth for each line in the CST's
/// own text (one entry per `\n`-terminated line, plus a trailing entry
/// for the final unterminated line). `None` means the line passes
/// through verbatim (whitespace-only, or block-scalar interior).
///
/// This is decoupled from the buffer so that rule 8 (which can shrink
/// inline whitespace and shift per-line byte counts) doesn't invalidate
/// rule 1's CST-offset lookup. The buffer-side pass
/// (`apply_canonical_indents`) iterates lines in parallel — rule 8
/// preserves `\n` positions, so the buffer's line count matches the
/// CST's.
fn precompute_line_depths(root: &SyntaxNode) -> Vec<Option<usize>> {
    let text = root.text().to_string();
    let mut out = Vec::new();
    let mut line_start = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed_start = line
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(line.len());
        let depth = if trimmed_start == line.len() {
            None
        } else {
            canonical_indent_depth(root, line_start + trimmed_start)
        };
        out.push(depth);
        line_start += line.len();
    }
    out
}

/// STYLE.md rule 1: every content line is indented by `2 * depth`
/// spaces, where `depth` counts the line's containing
/// `YAML_BLOCK_MAP_ENTRY` + `YAML_BLOCK_SEQUENCE_ITEM` ancestors
/// (root-level entries/items are depth 1 → 0-space indent). Depths are
/// precomputed from CST text by [`precompute_line_depths`]; this pass
/// iterates buffer lines in lockstep and rewrites only the leading-WS
/// slice of each line. Block-scalar interior lines and whitespace-only
/// lines (`depth == None`) pass through verbatim.
fn apply_canonical_indents(raw: &str, depths: &[Option<usize>]) -> String {
    let mut out = String::with_capacity(raw.len());
    for (i, line) in raw.split_inclusive('\n').enumerate() {
        let trimmed_start = line
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(line.len());
        if trimmed_start == line.len() {
            out.push_str(line);
            continue;
        }
        match depths.get(i).copied().flatten() {
            Some(depth) => {
                for _ in 0..depth {
                    out.push_str("  ");
                }
                out.push_str(&line[trimmed_start..]);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Compute the canonical indent depth (number of 2-space steps) for
/// the line whose first non-whitespace byte sits at `offset`.
/// Returns `None` if the line is the interior of a block scalar — in
/// that case the caller preserves the original indent.
fn canonical_indent_depth(root: &SyntaxNode, offset: usize) -> Option<usize> {
    let offset_ts = TextSize::try_from(offset).ok()?;
    let token = match root.token_at_offset(offset_ts) {
        TokenAtOffset::Single(t) => t,
        TokenAtOffset::Between(_, right) => right,
        TokenAtOffset::None => return Some(0),
    };

    if token.kind() == SyntaxKind::YAML_SCALAR {
        let text = token.text();
        let starts_block_scalar = text.starts_with('|') || text.starts_with('>');
        if starts_block_scalar && text.contains('\n') {
            let scalar_start = usize::from(token.text_range().start());
            if offset > scalar_start {
                return None;
            }
        }
    }

    let mut entry_item_ancestors = 0usize;
    let mut node = token.parent();
    while let Some(n) = node {
        if matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_ENTRY | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
        ) {
            entry_item_ancestors += 1;
        }
        node = n.parent();
    }
    Some(entry_item_ancestors.saturating_sub(1))
}

/// STYLE.md rule 10: strip trailing ASCII space + tab from every
/// line. Applied uniformly, including inside `|`/`>` block scalars
/// (pretty_yaml does the same; this trades semantic strictness in
/// literal blocks for the "no trailing whitespace anywhere" invariant
/// that the spec pins). `\r` is preserved so CRLF line endings round
/// trip.
fn strip_trailing_whitespace_per_line(buf: String) -> String {
    if !buf.contains([' ', '\t']) {
        return buf;
    }
    let mut out = String::with_capacity(buf.len());
    for (i, line) in buf.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end_matches([' ', '\t']));
    }
    out
}

/// STYLE.md rule 7: runs of multiple interior blank lines collapse to
/// one max; leading blank lines are stripped entirely (mirroring
/// rule 13's "no trailing blank lines" — preamble whitespace before the
/// first content line is never meaningful). Applied after rule 10 so
/// that whitespace-only "blank" lines (which rule 10 reduces to empty)
/// participate uniformly. A blank line is an empty `""` slot in the
/// `\n`-split (or `"\r"` for a CRLF-only line).
fn collapse_blank_line_runs(buf: String) -> String {
    let lines: Vec<&str> = buf.split('\n').collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut prev_blank = false;
    let mut seen_content = false;
    for line in lines {
        let is_blank = line.is_empty() || line == "\r";
        if is_blank && !seen_content {
            continue;
        }
        if is_blank && prev_blank {
            continue;
        }
        kept.push(line);
        prev_blank = is_blank;
        if !is_blank {
            seen_content = true;
        }
    }
    kept.join("\n")
}

/// STYLE.md rule 13: a successfully-formatted document ends with
/// exactly one `\n`. Missing → add one; many → collapse to one.
fn normalize_trailing_newline(mut buf: String) -> String {
    let trimmed_len = buf.trim_end_matches('\n').len();
    buf.truncate(trimmed_len);
    buf.push('\n');
    buf
}
