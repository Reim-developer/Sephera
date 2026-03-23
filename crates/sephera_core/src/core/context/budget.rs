#[derive(Debug, Clone, Copy)]
pub(super) struct ContextBudget {
    total: u64,
    metadata: u64,
    excerpts: u64,
}

impl ContextBudget {
    #[must_use]
    pub(super) const fn new(total_tokens: u64) -> Self {
        let metadata_tokens = total_tokens.div_ceil(10);
        let excerpt_tokens = total_tokens.saturating_sub(metadata_tokens);

        Self {
            total: total_tokens,
            metadata: metadata_tokens,
            excerpts: excerpt_tokens,
        }
    }

    #[must_use]
    pub(super) const fn total_tokens(self) -> u64 {
        self.total
    }

    #[must_use]
    pub(super) const fn metadata_tokens(self) -> u64 {
        self.metadata
    }

    #[must_use]
    pub(super) const fn excerpt_tokens(self) -> u64 {
        self.excerpts
    }
}

#[must_use]
pub(super) const fn estimate_tokens_from_bytes(bytes: u64) -> u64 {
    if bytes == 0 { 1 } else { bytes.div_ceil(4) }
}

#[must_use]
pub(super) fn estimate_metadata_tokens(
    language_count: usize,
    selected_file_count: usize,
    focus_count: usize,
    reserved_tokens: u64,
) -> u64 {
    let estimated = 128_u64
        + u64::try_from(language_count).unwrap_or(u64::MAX) * 16
        + u64::try_from(selected_file_count).unwrap_or(u64::MAX) * 24
        + u64::try_from(focus_count).unwrap_or(u64::MAX) * 16;

    estimated.min(reserved_tokens)
}
