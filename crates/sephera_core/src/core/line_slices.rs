use memchr::memchr2;

pub(super) struct LineSlices<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> LineSlices<'a> {
    #[must_use]
    pub(super) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }
}

impl<'a> Iterator for LineSlices<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.bytes.len() {
            return None;
        }

        let start = self.cursor;
        let rest = &self.bytes[start..];

        // `memchr2` over the remaining bytes rather than a byte-at-a-time loop.
        // Splitting lines is the first thing every scanner does with a file, and
        // at one comparison per byte it set the floor for everything after it:
        // the comment classifier was measured at a fraction of the speed of the
        // one that does no classification at all, for no reason other than how
        // the line boundaries were found.
        let Some(offset) = memchr2(b'\n', b'\r', rest) else {
            self.cursor = self.bytes.len();
            return Some(rest);
        };

        let end = start + offset;
        // A `\r` that is not followed by `\n` ends the line on its own, and a
        // `\r\n` pair is one terminator rather than an empty line between two.
        self.cursor = if self.bytes[end] == b'\n' {
            end + 1
        } else if end + 1 < self.bytes.len() && self.bytes[end + 1] == b'\n' {
            end + 2
        } else {
            end + 1
        };

        Some(&self.bytes[start..end])
    }
}
