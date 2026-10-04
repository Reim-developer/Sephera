//! A line range inside one source file.
//!
//! Exists so a context pack can be narrowed to a single declaration instead of
//! a whole file. Without it, `--focus-symbol` could only point at a file, which
//! still pulls in every unrelated function in it.

use std::ops::RangeInclusive;

/// A 1-based inclusive line range.
///
/// Both bounds are inclusive and 1-based to match what an editor shows, so a
/// range can be pasted straight from a file view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct LineRange {
    /// First line of the range, 1-based.
    pub start: usize,
    /// Last line of the range, 1-based and inclusive.
    pub end: usize,
}

impl LineRange {
    /// Build a range, clamping `end` to be at least `start`.
    ///
    /// A caller measuring a one-line declaration can otherwise produce an
    /// inverted range, which would select nothing and look like a parse bug.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        if end > start {
            Self { start, end }
        } else {
            Self { start, end: start }
        }
    }

    /// Whether this range covers `line`.
    #[must_use]
    pub const fn contains(&self, line: usize) -> bool {
        line >= self.start && line <= self.end
    }

    /// Number of lines the range spans, always at least one.
    #[must_use]
    pub const fn line_count(&self) -> usize {
        if self.end >= self.start {
            self.end - self.start + 1
        } else {
            1
        }
    }

    /// The range as a 0-based slice index range.
    #[must_use]
    pub const fn to_index_range(&self) -> RangeInclusive<usize> {
        // Line numbers are 1-based; slice indices are 0-based.
        self.start.saturating_sub(1)..=self.end.saturating_sub(1)
    }

    /// The same range shifted down by `offset` lines, floored at zero.
    ///
    /// Used when a caller works from byte offsets that are already relative to
    /// a slice.
    #[must_use]
    pub const fn shifted_by(&self, offset: usize) -> Self {
        let start = if self.start > offset {
            self.start - offset
        } else {
            1
        };
        let end = if self.end > offset {
            self.end - offset
        } else {
            1
        };
        Self::new(start, end)
    }

    /// Whether two ranges touch or overlap, so they cannot be emitted apart.
    ///
    /// Ranges exactly one line apart are contiguous: `1-3` and `4-6` cover
    /// lines 1 through 6 with no gap, so merging them keeps the excerpt
    /// continuous instead of inserting a separator between them.
    #[must_use]
    pub const fn touches(&self, other: &Self) -> bool {
        self.start <= other.end.saturating_add(1)
            && other.start <= self.end.saturating_add(1)
    }

    /// The smallest range covering both inputs.
    #[must_use]
    pub const fn union(&self, other: &Self) -> Self {
        let start = if self.start < other.start {
            self.start
        } else {
            other.start
        };
        let end = if self.end > other.end {
            self.end
        } else {
            other.end
        };
        Self { start, end }
    }

    /// Sort ranges by position and merge any that touch or overlap.
    ///
    /// Two requests for overlapping spans of one file are one span to emit;
    /// keeping both would duplicate the shared lines.
    #[must_use]
    pub fn normalized(ranges: &[Self]) -> Vec<Self> {
        let mut sorted = ranges.to_vec();
        sorted.sort_by_key(|range| (range.start, range.end));

        let mut merged: Vec<Self> = Vec::with_capacity(sorted.len());
        for range in sorted {
            match merged.last_mut() {
                Some(last) if last.touches(&range) => {
                    *last = last.union(&range);
                }
                _ => merged.push(range),
            }
        }
        merged
    }

    /// Total lines covered, counting each line once.
    ///
    /// Overlaps are counted once, so this is not the sum of [`Self::line_count`]
    /// when the input overlaps.
    #[must_use]
    pub fn total_line_count(ranges: &[Self]) -> usize {
        Self::normalized(ranges).iter().map(Self::line_count).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_line_range_spans_one_line() {
        let range = LineRange::new(7, 7);

        assert_eq!(range.line_count(), 1);
        assert!(range.contains(7));
        assert!(!range.contains(6));
        assert!(!range.contains(8));
    }

    #[test]
    fn an_inverted_range_is_clamped_rather_than_empty() {
        // Measuring a one-line declaration can yield end < start; an empty range
        // would select nothing and read like a parse failure.
        let range = LineRange::new(7, 3);

        assert_eq!(range.start, 7);
        assert_eq!(range.end, 7);
        assert!(range.contains(7));
    }

    #[test]
    fn multi_line_range_counts_inclusively() {
        let range = LineRange::new(3, 9);

        assert_eq!(range.line_count(), 7);
        assert!(range.contains(3));
        assert!(range.contains(9));
        assert!(!range.contains(2));
        assert!(!range.contains(10));
    }

    #[test]
    fn index_range_is_zero_based() {
        assert_eq!(LineRange::new(1, 1).to_index_range(), 0..=0);
        assert_eq!(LineRange::new(3, 5).to_index_range(), 2..=4);
    }

    #[test]
    fn shifting_floors_at_the_first_line() {
        let shifted = LineRange::new(3, 8).shifted_by(10);

        assert_eq!(shifted.start, 1);
        assert_eq!(shifted.end, 1, "a range above the offset collapses");
    }

    #[test]
    fn shifting_a_head_range_keeps_its_length() {
        let shifted = LineRange::new(10, 20).shifted_by(5);

        assert_eq!(shifted.start, 5);
        assert_eq!(shifted.end, 15);
    }

    #[test]
    fn adjacent_ranges_touch_but_distant_ones_do_not() {
        let range = LineRange::new(1, 3);

        assert!(
            range.touches(&LineRange::new(4, 6)),
            "no gap between 3 and 4"
        );
        assert!(range.touches(&LineRange::new(2, 8)), "overlapping");
        assert!(
            !range.touches(&LineRange::new(5, 6)),
            "line 4 is a real gap"
        );
    }

    #[test]
    fn union_covers_both_ranges() {
        let union = LineRange::new(2, 4).union(&LineRange::new(9, 11));

        assert_eq!(union.start, 2);
        assert_eq!(union.end, 11);
    }

    #[test]
    fn normalizing_sorts_and_merges() {
        let merged = LineRange::normalized(&[
            LineRange::new(9, 12),
            LineRange::new(1, 3),
            LineRange::new(4, 5),
            LineRange::new(11, 20),
        ]);

        assert_eq!(
            merged,
            vec![LineRange::new(1, 5), LineRange::new(9, 20)],
            "1-3 and 4-5 are contiguous; 9-12 overlaps 11-20"
        );
    }

    #[test]
    fn normalizing_an_empty_list_yields_nothing() {
        assert!(LineRange::normalized(&[]).is_empty());
    }

    #[test]
    fn normalizing_deduplicates_an_exact_repeat() {
        let merged = LineRange::normalized(&[
            LineRange::new(4, 8),
            LineRange::new(4, 8),
        ]);

        assert_eq!(merged, vec![LineRange::new(4, 8)]);
    }

    #[test]
    fn total_line_count_does_not_double_count_an_overlap() {
        let ranges = [LineRange::new(1, 10), LineRange::new(5, 15)];

        assert_eq!(LineRange::total_line_count(&ranges), 15);
        assert_eq!(
            LineRange::total_line_count(&[
                LineRange::new(1, 3),
                LineRange::new(10, 12)
            ]),
            6,
            "a gap is not counted"
        );
    }
}
