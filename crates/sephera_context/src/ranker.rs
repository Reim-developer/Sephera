use std::collections::{BTreeMap, BTreeSet};

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
        SelectionClass::DiffFile => 1,
        SelectionClass::FocusedDescendant => 2,
        SelectionClass::Manifest => 3,
        SelectionClass::Workflow => 4,
        SelectionClass::Entrypoint => 5,
        SelectionClass::AdjacentTest => 6,
        SelectionClass::General => 7,
    }
}

/// Reorder ranked candidates so a file's in-project dependencies
/// stand directly behind it.
///
/// Ranking alone packs a file and leaves what it imports to the
/// general competition, where a dependency competes with every
/// unrelated file of its class on size -- and loses, often, to
/// files nothing in the pack needs. A pack that does that is a
/// bag of files wearing a graph's clothes: the model reading it
/// holds the caller and not the callee. The closure pass walks the
/// ranked list and, after each candidate, emits its dependency
/// chain depth-first -- the files it imports, then theirs -- so
/// selection reaches a file's dependencies before any unrelated
/// file that ranked below the file itself. Under a budget that
/// runs out mid-chain, what is cut is the chain's tail or the
/// unrelated files, never a dependency passed over for an
/// unrelated file its importer outranked.
///
/// Returns the reordered candidates and the set of paths the pass
/// emitted as a dependency (rather than in their ranked place), so
/// the report can count how much of the pack the graph pulled in.
pub(super) fn interleave_dependency_closure(
    candidates: Vec<ContextCandidate>,
    dependencies: &BTreeMap<String, Vec<String>>,
) -> (Vec<ContextCandidate>, BTreeSet<String>) {
    let positions: BTreeMap<String, usize> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            (candidate.normalized_relative_path.clone(), index)
        })
        .collect();
    let ranked_paths: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.normalized_relative_path.clone())
        .collect();
    let mut slots: Vec<Option<ContextCandidate>> =
        candidates.into_iter().map(Some).collect();
    let mut order: Vec<ContextCandidate> = Vec::with_capacity(slots.len());
    let mut pulled: BTreeSet<String> = BTreeSet::new();
    let mut emitted: BTreeSet<String> = BTreeSet::new();

    for path in &ranked_paths {
        emit_chain(
            path,
            false,
            dependencies,
            &positions,
            &mut slots,
            &mut emitted,
            &mut pulled,
            &mut order,
        );
    }

    (order, pulled)
}

/// Emit one candidate and then, depth-first, the candidates it
/// imports. A candidate already emitted -- as an earlier file's
/// dependency, or in its own ranked place -- is not emitted twice;
/// the first claim on its position wins, which is the ranked file
/// unless a higher-ranked file's chain reached it first.
#[allow(
    clippy::too_many_arguments,
    reason = "the walk's whole state travels together; bundling it into a \
              struct would name the same eight things once more without \
              grouping any of them"
)]
fn emit_chain(
    path: &str,
    as_dependency: bool,
    dependencies: &BTreeMap<String, Vec<String>>,
    positions: &BTreeMap<String, usize>,
    slots: &mut [Option<ContextCandidate>],
    emitted: &mut BTreeSet<String>,
    pulled: &mut BTreeSet<String>,
    order: &mut Vec<ContextCandidate>,
) {
    if !emitted.insert(path.to_owned()) {
        return;
    }
    let Some(&index) = positions.get(path) else {
        return;
    };
    let Some(candidate) = slots[index].take() else {
        return;
    };
    if as_dependency {
        pulled.insert(path.to_owned());
    }
    order.push(candidate);

    if let Some(imports) = dependencies.get(path) {
        for imported in imports {
            emit_chain(
                imported,
                true,
                dependencies,
                positions,
                slots,
                emitted,
                pulled,
                order,
            );
        }
    }
}
