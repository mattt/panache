//! In-tree YAML formatter (shadow, Phase 1 of the cutover plan).
//!
//! Consumes the in-tree parser CST
//! ([`panache_parser::parser::yaml::parse_yaml_tree`]) and emits
//! deterministically-styled YAML text per the 13 style rules in
//! `STYLE.md` (next to this file) — the canonical spec, relocated
//! from `.claude/skills/yaml-formatter-cutover/plan.md` in Phase 1.2.
//!
//! **Not wired into the live formatting pipeline.** Until the joint
//! cutover lands (Phase 2), live YAML output still routes through
//! [`crate::yaml_engine`] → `pretty_yaml`. This module is
//! cross-validated against pretty_yaml on a corpus by
//! `crates/panache-formatter/tests/yaml_cross_validation.rs`. See
//! `.claude/skills/yaml-formatter-cutover/SKILL.md` for scope.
//!
//! Phase 1.3 status: cross-validation harness landed. The dispatcher
//! still walks the CST and emits every token's source byte verbatim
//! (byte-lossless, no style rules applied yet), so the corpus is
//! currently seeded only with trivially-canonical inputs that
//! round-trip identically through pretty_yaml's defaults. As the
//! per-container renderers land, real frontmatter and rule-exercising
//! stressors graduate into the corpus.

#[path = "yaml/block_map.rs"]
mod block_map;
#[path = "yaml/block_sequence.rs"]
mod block_sequence;
#[path = "yaml/document.rs"]
mod document;
#[path = "yaml/flow.rs"]
mod flow;
#[path = "yaml/options.rs"]
mod options;
#[path = "yaml/scalar.rs"]
mod scalar;

pub use options::{WrapMode, YamlFormatOptions};

/// Format the given YAML source under the in-tree formatter.
///
/// On a parse error (input the in-tree parser rejects outright),
/// returns the input verbatim. The cross-validation harness treats
/// that as a "skip" case — pretty_yaml also passes its input through
/// on its own rejection path, and a shadow formatter shouldn't be
/// the thing that surfaces parse errors.
pub fn format_yaml(input: &str, opts: &YamlFormatOptions) -> String {
    match panache_parser::parser::yaml::parse_yaml_tree(input) {
        Some(tree) => document::render(&tree, opts),
        None => input.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_round_trips() {
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("", &opts), "");
    }

    #[test]
    fn simple_mapping_is_byte_lossless_in_phase_1_1_stub() {
        // The 1.1 stub emits tokens verbatim. Cross-validation
        // against pretty_yaml lands in 1.3 and will surface the
        // style-rule gaps these byte-passthrough outputs leave open.
        let opts = YamlFormatOptions::default();
        let input = "title: My Title\nauthor: Me\n";
        assert_eq!(format_yaml(input, &opts), input);
    }
}
