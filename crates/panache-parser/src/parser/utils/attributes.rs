//! Parsing for Pandoc-style attributes: {#id .class key=value}
//!
//! Attributes can appear after headings, fenced code blocks, fenced divs, etc.
//! Syntax: {#identifier .class1 .class2 key1=val1 key2="val2"}
//!
//! Rules:
//! - Surrounded by { }
//! - Identifier: #id (optional, only first one counts)
//! - Classes: .class (can have multiple)
//! - Key-value pairs: key=value or key="value" or key='value' (can have multiple)
//! - Whitespace flexible between items

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

#[derive(Debug, PartialEq)]
pub struct AttributeBlock {
    pub identifier: Option<String>,
    pub classes: Vec<String>,
    pub key_values: Vec<(String, String)>,
}

/// Try to parse an attribute block from the end of a string
/// Returns: (attribute_block, text_before_attributes)
pub fn try_parse_trailing_attributes(text: &str) -> Option<(AttributeBlock, &str)> {
    let (attrs, before, _) = try_parse_trailing_attributes_with_pos(text)?;
    Some((attrs, before))
}

/// Try to parse an attribute block from the end of a string.
/// Returns: (attribute_block, text_before_attributes, open_brace_position_in_trimmed_text)
pub fn try_parse_trailing_attributes_with_pos(text: &str) -> Option<(AttributeBlock, &str, usize)> {
    let trimmed = text.trim_end();

    // Must end with }
    if !trimmed.ends_with('}') {
        return None;
    }

    // Find matching opening brace for the trailing attribute block, accounting
    // for braces inside quoted attribute values.
    let open_brace = find_matching_open_brace_for_trailing_block(trimmed)?;

    // Check if this is a bracketed span like [text]{.class} rather than a heading attribute
    // If the { is immediately after ] (with optional whitespace), this should be parsed as a span
    let before_brace = &trimmed[..open_brace];
    if before_brace.trim_end().ends_with(']') {
        log::trace!("Skipping attribute parsing for bracketed span: {}", text);
        return None;
    }

    // Parse the content between { and }
    let attr_content = &trimmed[open_brace + 1..trimmed.len() - 1];
    let attr_block = parse_attribute_content(attr_content)?;

    // Get text before attributes (trim trailing whitespace)
    let before_attrs = trimmed[..open_brace].trim_end();

    Some((attr_block, before_attrs, open_brace))
}

fn find_matching_open_brace_for_trailing_block(text: &str) -> Option<usize> {
    if !text.ends_with('}') {
        return None;
    }

    let mut stack: Vec<usize> = Vec::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    let mut end_brace_open = None;

    for (idx, ch) in text.char_indices() {
        if let Some(q) = in_quote {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == q {
                in_quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => in_quote = Some(ch),
            '{' => stack.push(idx),
            '}' => {
                let open = stack.pop()?;
                if idx == text.len() - 1 {
                    end_brace_open = Some(open);
                }
            }
            _ => {}
        }
    }

    if in_quote.is_some() || !stack.is_empty() {
        return None;
    }

    end_brace_open
}

/// One recognized component inside an attribute `{...}` body, as byte ranges
/// relative to the `content` slice passed to [`attribute_content_spans`] (the
/// bytes strictly between `{` and `}`). Marker bytes (`#`/`.`/`=`) and value
/// quotes are kept INSIDE the ranges so the emitter can wrap the exact source
/// bytes; the string-deriving helpers strip them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AttrComponent {
    /// `#id` — range includes the leading `#`.
    Id(std::ops::Range<usize>),
    /// `.class` or `=format` — range includes the leading `.`/`=` marker.
    Class(std::ops::Range<usize>),
    /// `key=value`: key range, `=` byte index, value range (the value range
    /// includes surrounding quotes when present).
    KeyValue {
        key: std::ops::Range<usize>,
        eq: usize,
        value: std::ops::Range<usize>,
    },
}

/// Recognized components of an attribute `{...}` body, in source order. The
/// single source of truth shared by detection ([`parse_attribute_content`],
/// which derives owned strings) and emission (`emit_attribute_node`, which
/// wraps these byte ranges in ATTR_* CST nodes) — one walk, no detect/emit
/// drift. Bytes the scan skips (duplicate `#id`, malformed tokens, whitespace)
/// are not components; the emitter recovers them from the gaps between ranges.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttributeSpans {
    pub components: Vec<AttrComponent>,
}

/// Strip a matching pair of surrounding quotes (`"` or `'`) from an attribute
/// value's raw bytes, yielding the semantic value. Mirrors the quote handling
/// in the legacy [`parse_attribute_content`] walk: a leading quote is always
/// dropped, and a trailing quote of the same kind is dropped when present (so
/// unterminated quotes keep their tail).
fn attr_value_string(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if let Some(&q) = bytes.first()
        && (q == b'"' || q == b'\'')
    {
        let inner = &raw[1..];
        return inner.strip_suffix(q as char).unwrap_or(inner).to_string();
    }
    raw.to_string()
}

/// Scan an attribute `{...}` body into [`AttributeSpans`]. Returns `None` when
/// no component is recognized (empty/whitespace-only/`{}` is not a valid
/// attribute block). Offsets are relative to `content`.
pub(crate) fn attribute_content_spans(content: &str) -> Option<AttributeSpans> {
    let bytes = content.as_bytes();
    let mut pos = 0;
    let mut components: Vec<AttrComponent> = Vec::new();
    let mut have_id = false;

    while pos < bytes.len() {
        // Skip whitespace.
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        if bytes[pos] == b'=' {
            // {=format} raw-attribute marker — recorded as a class whose range
            // includes the `=` (the string derivation keeps the `=`).
            let start = pos;
            pos += 1; // skip '='
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            if pos > start + 1 {
                components.push(AttrComponent::Class(start..pos));
            }
        } else if bytes[pos] == b'#' {
            let start = pos;
            pos += 1; // skip '#'
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            // Only the first non-empty identifier counts; later `#…` runs and a
            // bare `#` are scanned but not recorded (recovered from the gap).
            if !have_id && pos > start + 1 {
                components.push(AttrComponent::Id(start..pos));
                have_id = true;
            }
        } else if bytes[pos] == b'.' {
            let start = pos;
            pos += 1; // skip '.'
            while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                pos += 1;
            }
            if pos > start + 1 {
                components.push(AttrComponent::Class(start..pos));
            }
        } else {
            // key=value
            let key_start = pos;
            while pos < bytes.len() && bytes[pos] != b'=' && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= bytes.len() || bytes[pos] != b'=' {
                // Not a valid key=value: skip the token (recovered from the gap).
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                continue;
            }
            let key_end = pos;
            let eq = pos;
            pos += 1; // skip '='

            let value_start = pos;
            if pos < bytes.len() && (bytes[pos] == b'"' || bytes[pos] == b'\'') {
                let quote = bytes[pos];
                pos += 1; // opening quote
                while pos < bytes.len() && bytes[pos] != quote {
                    pos += 1;
                }
                if pos < bytes.len() {
                    pos += 1; // closing quote
                }
            } else {
                while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'}' {
                    pos += 1;
                }
            }
            if key_end > key_start {
                components.push(AttrComponent::KeyValue {
                    key: key_start..key_end,
                    eq,
                    value: value_start..pos,
                });
            }
        }
    }

    if components.is_empty() {
        return None;
    }
    Some(AttributeSpans { components })
}

/// Parse the content inside the attribute braces into owned strings. Thin
/// wrapper over [`attribute_content_spans`] so detection and emission share one
/// walk.
pub fn parse_attribute_content(content: &str) -> Option<AttributeBlock> {
    let spans = attribute_content_spans(content)?;
    let mut identifier = None;
    let mut classes = Vec::new();
    let mut key_values = Vec::new();

    for comp in &spans.components {
        match comp {
            AttrComponent::Id(r) => {
                // Range includes '#'; the scanner guarantees a non-empty tail.
                identifier = Some(content[r.start + 1..r.end].to_string());
            }
            AttrComponent::Class(r) => {
                let raw = &content[r.clone()];
                // `.class` → `class`; `=format` keeps its `=` prefix.
                match raw.strip_prefix('.') {
                    Some(class) => classes.push(class.to_string()),
                    None => classes.push(raw.to_string()),
                }
            }
            AttrComponent::KeyValue { key, value, .. } => {
                key_values.push((
                    content[key.clone()].to_string(),
                    attr_value_string(&content[value.clone()]),
                ));
            }
        }
    }

    Some(AttributeBlock {
        identifier,
        classes,
        key_values,
    })
}

/// Parse HTML-style attributes from a raw HTML opening tag text such as
/// `<div id="x" class="a b" data-key="v">`, returning the same
/// `AttributeBlock` shape as Pandoc-style brace attributes. Whitespace-
/// separated `class="..."` is split into individual classes; `id="..."`
/// becomes the identifier; everything else becomes a key/value pair.
/// Returns `None` if the tag has no recognized attributes.
///
/// Self-closing slashes (`<div .../>`) and trailing whitespace are tolerated.
/// The leading `<TAG` and trailing `>` are stripped; this routine does not
/// validate the tag name.
pub fn parse_html_tag_attributes(tag_text: &str) -> Option<AttributeBlock> {
    let trimmed = tag_text.trim_start();
    let after_lt = trimmed.strip_prefix('<')?;
    // Find the end of the opening tag at the first `>` not inside a quoted
    // attribute value. Anything after that `>` (e.g. inline content + close
    // tag for a same-line `<div id="x">Content</div>`) is irrelevant.
    let bytes = after_lt.as_bytes();
    let mut tag_end = None;
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match (quote, b) {
            (None, b'"') | (None, b'\'') => quote = Some(b),
            (Some(q), b2) if b2 == q => quote = None,
            (None, b'>') => {
                tag_end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let tag_end = tag_end?;
    let inner = &after_lt[..tag_end];
    // Drop any trailing self-closing slash.
    let inner = inner.trim_end().trim_end_matches('/').trim_end();
    // Drop the tag name (alphanumeric run after `<`).
    let bytes = inner.as_bytes();
    let mut name_end = 0usize;
    while name_end < bytes.len()
        && !bytes[name_end].is_ascii_whitespace()
        && bytes[name_end] != b'/'
    {
        name_end += 1;
    }
    let attrs_text = &inner[name_end..];
    parse_html_attribute_list(attrs_text)
}

/// Parse a raw HTML attribute list (the bytes between a tag name and the
/// closing `>`, exclusive). Accepts inputs like `id="x" class="a b"
/// data-key=v` and produces an [`AttributeBlock`]. Returns `None` if no
/// recognized attributes are present.
///
/// Used by [`parse_html_tag_attributes`] (which strips `<TAG ...>`
/// surrounding chrome before delegating here) and by
/// `AttributeNode::id` for the structural `HTML_ATTRS` CST node, whose
/// text holds JUST the attribute region.
pub fn parse_html_attribute_list(attrs_text: &str) -> Option<AttributeBlock> {
    let mut identifier: Option<String> = None;
    let mut classes: Vec<String> = Vec::new();
    let mut key_values: Vec<(String, String)> = Vec::new();

    let bytes = attrs_text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' | b'/' => {
                i += 1;
            }
            _ => {
                let key_start = i;
                while i < bytes.len()
                    && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'=' | b'/')
                {
                    i += 1;
                }
                let key = &attrs_text[key_start..i];
                let value = if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                        let quote = bytes[i];
                        i += 1;
                        let v_start = i;
                        while i < bytes.len() && bytes[i] != quote {
                            i += 1;
                        }
                        let v = attrs_text[v_start..i].to_string();
                        if i < bytes.len() {
                            i += 1;
                        }
                        v
                    } else {
                        let v_start = i;
                        while i < bytes.len()
                            && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'/')
                        {
                            i += 1;
                        }
                        attrs_text[v_start..i].to_string()
                    }
                } else {
                    String::new()
                };
                if key.is_empty() {
                    continue;
                }
                match key {
                    "id" => {
                        if identifier.is_none() && !value.is_empty() {
                            identifier = Some(value);
                        }
                    }
                    "class" => {
                        for c in value.split_ascii_whitespace() {
                            classes.push(c.to_string());
                        }
                    }
                    _ => key_values.push((key.to_string(), value)),
                }
            }
        }
    }

    if identifier.is_none() && classes.is_empty() && key_values.is_empty() {
        return None;
    }
    Some(AttributeBlock {
        identifier,
        classes,
        key_values,
    })
}

/// Emit a Pandoc `{...}` ATTRIBUTE node by STRUCTURING the raw source slice
/// into ATTR_* children that wrap the original bytes (no synthesis). Markers
/// and quotes stay inside their tokens; whitespace/newlines between components,
/// and any bytes the scanner skips (duplicate `#id`, malformed tokens), become
/// standalone WHITESPACE/NEWLINE/TEXT tokens — so `node.text()` is exactly the
/// source slice. Non-`{...}`-shaped or unrecognized input (MMD `[#id]` header
/// brackets, raw-inline `{=format}`, empty `{}`) falls back to a single opaque
/// ATTRIBUTE token, preserving the prior shape.
pub fn emit_attribute_node(builder: &mut GreenNodeBuilder, raw_attr_text: &str) {
    builder.start_node(SyntaxKind::ATTRIBUTE.into());

    let body = raw_attr_text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'));
    let spans = body.and_then(attribute_content_spans);

    match (body, spans) {
        (Some(body), Some(spans)) => {
            builder.token(SyntaxKind::TEXT.into(), "{");
            let mut cursor = 0usize;
            for comp in &spans.components {
                let (start, end) = match comp {
                    AttrComponent::Id(r) | AttrComponent::Class(r) => (r.start, r.end),
                    AttrComponent::KeyValue { key, value, .. } => (key.start, value.end),
                };
                emit_attribute_gap(builder, &body[cursor..start]);
                match comp {
                    AttrComponent::Id(r) => {
                        builder.token(SyntaxKind::ATTR_ID.into(), &body[r.clone()]);
                    }
                    AttrComponent::Class(r) => {
                        builder.token(SyntaxKind::ATTR_CLASS.into(), &body[r.clone()]);
                    }
                    AttrComponent::KeyValue { key, eq, value } => {
                        builder.start_node(SyntaxKind::ATTR_KEY_VALUE.into());
                        builder.token(SyntaxKind::ATTR_KEY.into(), &body[key.clone()]);
                        builder.token(SyntaxKind::TEXT.into(), &body[*eq..*eq + 1]);
                        if value.end > value.start {
                            builder.token(SyntaxKind::ATTR_VALUE.into(), &body[value.clone()]);
                        }
                        builder.finish_node();
                    }
                }
                cursor = end;
            }
            emit_attribute_gap(builder, &body[cursor..]);
            builder.token(SyntaxKind::TEXT.into(), "}");
        }
        _ => {
            // Opaque fallback: keep the whole slice as one ATTRIBUTE token.
            builder.token(SyntaxKind::ATTRIBUTE.into(), raw_attr_text);
        }
    }

    builder.finish_node();
}

/// Emit the bytes between/around structured attribute components, splitting on
/// newline boundaries: `\n`/`\r\n`/`\r` → NEWLINE, other whitespace runs →
/// WHITESPACE, non-whitespace runs → TEXT. Every byte is preserved.
fn emit_attribute_gap(builder: &mut GreenNodeBuilder, gap: &str) {
    let bytes = gap.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                builder.token(SyntaxKind::NEWLINE.into(), "\n");
                i += 1;
            }
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    builder.token(SyntaxKind::NEWLINE.into(), "\r\n");
                    i += 2;
                } else {
                    builder.token(SyntaxKind::NEWLINE.into(), "\r");
                    i += 1;
                }
            }
            b if b.is_ascii_whitespace() => {
                let start = i;
                while i < bytes.len()
                    && bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'\n'
                    && bytes[i] != b'\r'
                {
                    i += 1;
                }
                builder.token(SyntaxKind::WHITESPACE.into(), &gap[start..i]);
            }
            _ => {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                builder.token(SyntaxKind::TEXT.into(), &gap[start..i]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_id() {
        let result = try_parse_trailing_attributes("Heading {#my-id}");
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Heading");
        assert_eq!(attrs.identifier, Some("my-id".to_string()));
        assert!(attrs.classes.is_empty());
        assert!(attrs.key_values.is_empty());
    }

    #[test]
    fn test_single_class() {
        let result = try_parse_trailing_attributes("Text {.myclass}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.classes, vec!["myclass"]);
    }

    #[test]
    fn test_multiple_classes() {
        let result = try_parse_trailing_attributes("Text {.class1 .class2 .class3}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.classes, vec!["class1", "class2", "class3"]);
    }

    #[test]
    fn test_key_value_unquoted() {
        let result = try_parse_trailing_attributes("Text {key=value}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "value".to_string())]
        );
    }

    #[test]
    fn test_key_value_quoted() {
        let result = try_parse_trailing_attributes("Text {key=\"value with spaces\"}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "value with spaces".to_string())]
        );
    }

    #[test]
    fn test_full_attributes() {
        let result =
            try_parse_trailing_attributes("Heading {#id .class1 .class2 key1=val1 key2=\"val 2\"}");
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Heading");
        assert_eq!(attrs.identifier, Some("id".to_string()));
        assert_eq!(attrs.classes, vec!["class1", "class2"]);
        assert_eq!(attrs.key_values.len(), 2);
        assert_eq!(
            attrs.key_values[0],
            ("key1".to_string(), "val1".to_string())
        );
        assert_eq!(
            attrs.key_values[1],
            ("key2".to_string(), "val 2".to_string())
        );
    }

    #[test]
    fn test_trailing_attributes_with_shortcode_in_quoted_value() {
        let text = "Slide Title {background-image='{{< placeholder 100 100 >}}' background-size=\"100px\"}";
        let result = try_parse_trailing_attributes(text);
        assert!(result.is_some());
        let (attrs, before) = result.unwrap();
        assert_eq!(before, "Slide Title");
        assert_eq!(attrs.key_values.len(), 2);
        assert_eq!(
            attrs.key_values[0],
            (
                "background-image".to_string(),
                "{{< placeholder 100 100 >}}".to_string()
            )
        );
        assert_eq!(
            attrs.key_values[1],
            ("background-size".to_string(), "100px".to_string())
        );
    }

    #[test]
    fn test_no_attributes() {
        let result = try_parse_trailing_attributes("Heading with no attributes");
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_braces() {
        let result = try_parse_trailing_attributes("Heading {}");
        assert!(result.is_none());
    }

    #[test]
    fn test_only_first_id_counts() {
        let result = try_parse_trailing_attributes("Text {#id1 #id2}");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.identifier, Some("id1".to_string()));
    }

    #[test]
    fn test_whitespace_handling() {
        let result = try_parse_trailing_attributes("Text {  #id   .class   key=val  }");
        assert!(result.is_some());
        let (attrs, _) = result.unwrap();
        assert_eq!(attrs.identifier, Some("id".to_string()));
        assert_eq!(attrs.classes, vec!["class"]);
        assert_eq!(
            attrs.key_values,
            vec![("key".to_string(), "val".to_string())]
        );
    }

    #[test]
    fn test_parse_html_tag_attributes_id_only() {
        let attrs = parse_html_tag_attributes(r#"<div id="anchor-c">"#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("anchor-c"));
        assert!(attrs.classes.is_empty());
        assert!(attrs.key_values.is_empty());
    }

    #[test]
    fn test_parse_html_tag_attributes_inline_content_after_open() {
        // For a same-line block `<div id="x">Content</div>`, the entire
        // line is in the HTML_BLOCK_TAG. The parser must terminate at the
        // first unquoted `>` and ignore the trailing content + close tag.
        let attrs = parse_html_tag_attributes(r#"<div id="anchor-c">Content.</div>"#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("anchor-c"));
    }

    #[test]
    fn test_parse_html_tag_attributes_class_and_kv() {
        let attrs = parse_html_tag_attributes(r#"<div id="x" class="a b" data-key="v">"#).unwrap();
        assert_eq!(attrs.identifier.as_deref(), Some("x"));
        assert_eq!(attrs.classes, vec!["a", "b"]);
        assert_eq!(
            attrs.key_values,
            vec![("data-key".to_string(), "v".to_string())]
        );
    }

    #[test]
    fn test_parse_html_tag_attributes_no_attrs() {
        assert!(parse_html_tag_attributes("<div>").is_none());
    }

    #[test]
    fn test_trailing_whitespace_before_attrs() {
        let result = try_parse_trailing_attributes("Heading   {#id}");
        assert!(result.is_some());
        let (_, before) = result.unwrap();
        assert_eq!(before, "Heading");
    }

    /// Regression: the inline-code attribute path used to reconstruct a
    /// normalized `{...}` string (reordering id-first, force-quoting values),
    /// which inflated the CST past the input and broke losslessness. The
    /// structured emitter must wrap the original bytes verbatim.
    #[test]
    fn inline_code_attribute_is_lossless() {
        let input = "`code`{.r #x key=v}\n";
        let tree = crate::parse(input, None);
        assert_eq!(tree.text().to_string(), input);
    }

    fn structured_attr(raw: &str) -> crate::syntax::SyntaxNode {
        let mut builder = GreenNodeBuilder::new();
        emit_attribute_node(&mut builder, raw);
        crate::syntax::SyntaxNode::new_root(builder.finish())
    }

    #[test]
    fn emit_attribute_node_is_lossless_over_shapes() {
        // Interior whitespace, duplicate id, malformed/empty bodies, mixed
        // quotes, and `=format` must all round-trip byte-for-byte.
        for raw in [
            "{#id}",
            "{.a .b}",
            "{key=\"v w\"}",
            "{ #id  .c }",
            "{#id1 #id2}",
            "{key}",
            "{=html}",
            "{#id .a key=v key2='x'}",
            "{key=}",
            "{}",
            "{   }",
        ] {
            let node = structured_attr(raw);
            assert_eq!(node.text().to_string(), raw, "lossless emit for {raw:?}");
            assert_eq!(node.kind(), SyntaxKind::ATTRIBUTE);
        }
    }

    #[test]
    fn emit_attribute_node_structures_children() {
        let node = structured_attr("{#x .a .b k=v}");
        let kinds: Vec<_> = node.children_with_tokens().map(|c| c.kind()).collect();
        assert_eq!(
            kinds.iter().filter(|k| **k == SyntaxKind::ATTR_ID).count(),
            1
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_CLASS)
                .count(),
            2
        );
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == SyntaxKind::ATTR_KEY_VALUE)
                .count(),
            1
        );
    }
}
