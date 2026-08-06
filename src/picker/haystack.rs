//! Per-entry preprocessing for nucleo matching plus the mapping that turns
//! nucleo's highlight char-positions (in the normalized string) back into byte
//! offsets in the original `~/brain/...` display string.

use std::collections::BTreeSet;

/// Per-entry preprocessing for nucleo matching + highlight mapping.
pub(super) struct HaystackBuf {
    /// The display string with slug separators (`-`, `_`, `.`) stripped.
    /// Nucleo matches against this, so word atoms like `afloat` find
    /// slugs like `ann-afloat` without the dashes splitting the run.
    pub(super) normalized: String,
    /// For each char position in `normalized`, the byte offset of the same
    /// char in the original `Entry::display`. Built once at startup so
    /// nucleo's highlight indices (char positions in the normalized
    /// `Utf32Str`) translate cheaply to display byte offsets at render time.
    pub(super) normalized_char_to_display_byte: Vec<usize>,
}

impl HaystackBuf {
    pub(super) fn new(display: &str) -> Self {
        let mut normalized = String::with_capacity(display.len());
        let mut map = Vec::with_capacity(display.len());
        for (byte_idx, ch) in display.char_indices() {
            if !matches!(ch, '-' | '_' | '.') {
                normalized.push(ch);
                map.push(byte_idx);
            }
        }
        Self {
            normalized,
            normalized_char_to_display_byte: map,
        }
    }
}

pub(super) fn char_positions_to_byte_positions(
    char_positions: &[u32],
    char_to_byte: &[usize],
) -> BTreeSet<usize> {
    char_positions
        .iter()
        .filter_map(|&cp| char_to_byte.get(cp as usize).copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haystack_strips_slug_separators() {
        let h = HaystackBuf::new("ann-afloat_v.2");
        assert_eq!(h.normalized, "annafloatv2");
    }

    #[test]
    fn haystack_char_to_byte_map_round_trips() {
        let display = "a-b_c";
        let h = HaystackBuf::new(display);
        assert_eq!(h.normalized, "abc");
        // Each normalized char's recorded byte offset must point at the same
        // char in the original display string.
        for (norm_char_idx, ch) in h.normalized.chars().enumerate() {
            let byte = h.normalized_char_to_display_byte[norm_char_idx];
            assert_eq!(display[byte..].chars().next(), Some(ch));
        }
    }

    #[test]
    fn char_positions_map_to_display_bytes() {
        let h = HaystackBuf::new("ann-afloat");
        // "afloat" begins at normalized char index 3 ("ann" = 0,1,2).
        let positions = [3u32, 4, 5];
        let bytes =
            char_positions_to_byte_positions(&positions, &h.normalized_char_to_display_byte);
        // In the *display* string, 'a' of "afloat" sits after "ann-" → byte 4.
        assert!(bytes.contains(&4));
    }
}
