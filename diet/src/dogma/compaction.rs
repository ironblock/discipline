//! The native summary's compaction prompt (#936): Qwen Code's, reproduced
//! verbatim from `QwenLM/qwen-code` at `v0.25.3`, under Apache-2.0 --
//! `diet/dogma/compaction/README.md` carries its source and notice. The
//! file is pinned in `MANIFEST.tsv` as `compaction/qwen-code-v0.25.3`, as the
//! refutation tool is pinned (`super::refutation`).

use super::digest;

/// Qwen Code's compression prompt, exactly as pinned.
pub const QWEN_CODE: &str = include_str!("../../dogma/compaction/qwen-code-v0.25.3.txt");

/// Qwen Code's stub for a tool result its microcompaction cleared, exactly
/// as pinned (#1050): what the trunk carries in the place of a result the
/// operator wraps with no span. `MICROCOMPACT_CLEARED_MESSAGE`,
/// `packages/core/src/services/microcompaction/microcompact.ts:14` at the
/// same tag.
pub const QWEN_CODE_CLEARED: &str =
    include_str!("../../dogma/compaction/qwen-code-v0.25.3-cleared.txt");

/// The prompt's manifest line, as `MANIFEST.tsv` writes it.
#[must_use]
pub fn manifest_line() -> String {
    format!(
        "compaction/qwen-code-v0.25.3\t{}\t{}\n",
        digest(QWEN_CODE),
        QWEN_CODE.len()
    )
}

/// The cleared stub's manifest line, as `MANIFEST.tsv` writes it.
#[must_use]
pub fn cleared_manifest_line() -> String {
    format!(
        "compaction/qwen-code-v0.25.3-cleared\t{}\t{}\n",
        digest(QWEN_CODE_CLEARED),
        QWEN_CODE_CLEARED.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #936: the prompt is pinned -- its manifest line is the manifest's.
    #[test]
    fn the_compaction_prompt_is_pinned() {
        assert!(
            super::super::MANIFEST.contains(&manifest_line()),
            "MANIFEST.tsv lacks {}",
            manifest_line()
        );
        assert!(QWEN_CODE.starts_with("You are the component that summarizes"));
    }

    /// #1050: Qwen Code's cleared stub is pinned, byte for byte.
    #[test]
    fn the_cleared_stub_is_pinned() {
        assert!(
            super::super::MANIFEST.contains(&cleared_manifest_line()),
            "MANIFEST.tsv lacks {}",
            cleared_manifest_line()
        );
        assert_eq!(QWEN_CODE_CLEARED, "[Old tool result content cleared]");
    }
}
