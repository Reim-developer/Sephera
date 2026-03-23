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
        let mut end = self.cursor;

        while end < self.bytes.len() {
            match self.bytes[end] {
                b'\n' => {
                    self.cursor = end + 1;
                    return Some(&self.bytes[start..end]);
                }
                b'\r' => {
                    self.cursor = if end + 1 < self.bytes.len()
                        && self.bytes[end + 1] == b'\n'
                    {
                        end + 2
                    } else {
                        end + 1
                    };
                    return Some(&self.bytes[start..end]);
                }
                _ => {
                    end += 1;
                }
            }
        }

        self.cursor = self.bytes.len();
        Some(&self.bytes[start..end])
    }
}
