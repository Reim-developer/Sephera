use super::{candidate::ContextCandidate, types::SelectionClass};

pub(super) fn rank_candidates(candidates: &mut [ContextCandidate]) {
    candidates.sort_by(|left, right| {
        selection_priority(left.selection_class)
            .cmp(&selection_priority(right.selection_class))
            .then_with(|| left.size_bytes.cmp(&right.size_bytes))
            .then_with(|| {
                left.normalized_relative_path
                    .cmp(&right.normalized_relative_path)
            })
    });
}

const fn selection_priority(selection_class: SelectionClass) -> u8 {
    match selection_class {
        SelectionClass::FocusedFile => 0,
        SelectionClass::FocusedDescendant => 1,
        SelectionClass::Manifest => 2,
        SelectionClass::Workflow => 3,
        SelectionClass::Entrypoint => 4,
        SelectionClass::AdjacentTest => 5,
        SelectionClass::General => 6,
    }
}
