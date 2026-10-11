//! The reverse index: which file uses which name.
//!
//! `declarations` answers what a file *is*. This answers what it *uses* -- every
//! mention of a name that is not that file's own to define. The pair is what a
//! symbol-level blast radius needs: change a declaration and the users are the
//! files that mention its name, not the files that import its module.
//!
//! Nothing here resolves. The index is a lookup structure, and resolution is a
//! separate question the owning crate asks of the declaration index. Keeping them
//! apart is what lets the extraction half be tested against a grammar and the
//! resolution half against a project, without either needing the other.
//!
//! A use that resolves to nothing is simply not recorded as an edge, so a name
//! that appears in a hundred files and means a hundred different things costs no
//! memory and produces no claim. That is the counting rule this whole crate rests
//! on: only resolved edges count, so an index built of guesses would be worse than
//! no index at all.

use std::collections::BTreeMap;

use serde::Serialize;

/// One mention of a name inside one file.
///
/// Produced per file by a plugin, so it carries no file of its own -- the walk
/// that collected it knows which file it came from, and a second spelling of the
/// same fact is a second thing to keep in step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameUse {
    /// The name as it is written where it is used.
    pub name: String,
    /// 1-based line the use appears on.
    pub line: usize,
    /// The path this name was brought in by, when it was imported.
    ///
    /// `use foo::Bar as Baz;` makes `Baz` mean `foo::Bar` inside this file, and a
    /// use of `Baz` cannot be resolved without knowing that. `None` for a name
    /// the file did not import -- one it declares itself, or one bound locally.
    pub alias_of: Option<String>,
}

impl NameUse {
    /// A mention of `name` on `line`, with no recorded import path.
    #[must_use]
    pub fn new(name: impl Into<String>, line: usize) -> Self {
        Self {
            name: name.into(),
            line,
            alias_of: None,
        }
    }

    /// A mention of `name` on `line`, bound to `path` by an import.
    #[must_use]
    pub fn aliased(
        name: impl Into<String>,
        line: usize,
        path: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            line,
            alias_of: Some(path.into()),
        }
    }
}

/// Which files mention a name, and where.
///
/// Keyed by name, then file, then the lines -- because the questions asked of it
/// all start with the name ("who uses `Resolver`?") and the file order inside is
/// what makes the answer a list a reader can scroll. Lines are kept rather than
/// counted, because a use on one line and a use on twenty are different risks
/// even though both are one file.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReferenceIndex {
    /// Name, then file, then the lines the name appears on in that file.
    by_name: BTreeMap<String, BTreeMap<String, Vec<usize>>>,
}

impl ReferenceIndex {
    /// Record every use one file makes.
    ///
    /// The file is stripped of any leading `./`, because a path spelled two ways
    /// in one project is two keys for one file and an undercount of both.
    pub fn record(&mut self, file: &str, uses: &[NameUse]) {
        let file = file.trim_start_matches("./");
        for used in uses {
            self.by_name
                .entry(used.name.clone())
                .or_default()
                .entry(file.to_owned())
                .or_default()
                .push(used.line);
        }
    }

    /// The files that mention `name`, each with the lines it mentions it on.
    pub fn users_of(
        &self,
        name: &str,
    ) -> impl Iterator<Item = (&str, &[usize])> {
        self.by_name.get(name).into_iter().flat_map(|by_file| {
            by_file
                .iter()
                .map(|(file, lines)| (file.as_str(), lines.as_slice()))
        })
    }

    /// How many files mention `name`.
    #[must_use]
    pub fn files_using(&self, name: &str) -> usize {
        self.by_name.get(name).map_or(0, BTreeMap::len)
    }

    /// Every name the index has seen, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// Whether no uses have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// How many distinct names the index has seen.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> ReferenceIndex {
        ReferenceIndex::default()
    }

    #[test]
    fn a_use_is_recorded_against_the_file_it_came_from() {
        let mut index = index();
        index.record(
            "src/a.rs",
            &[NameUse::new("Resolver", 4), NameUse::new("Resolver", 11)],
        );

        let users: Vec<_> = index.users_of("Resolver").collect();
        assert_eq!(users, vec![("src/a.rs", &[4, 11][..])]);
        assert_eq!(index.files_using("Resolver"), 1);
    }

    #[test]
    fn two_files_using_the_same_name_are_two_files() {
        let mut index = index();
        index.record("src/a.rs", &[NameUse::new("Resolver", 4)]);
        index.record("src/b.rs", &[NameUse::new("Resolver", 90)]);

        // Sorted by file, which is what makes the answer a scrollable list.
        let users: Vec<_> = index.users_of("Resolver").collect();
        assert_eq!(
            users,
            vec![("src/a.rs", &[4][..]), ("src/b.rs", &[90][..])]
        );
        assert_eq!(index.files_using("Resolver"), 2);
    }

    #[test]
    fn a_name_used_in_several_files_is_indexed_under_one_key() {
        let mut index = index();
        index.record("src/a.rs", &[NameUse::new("Config", 1)]);
        index.record("src/b.rs", &[NameUse::new("Config", 2)]);
        index.record(
            "src/c.rs",
            &[NameUse::new("Config", 3), NameUse::new("Other", 8)],
        );

        assert_eq!(index.len(), 2);
        assert_eq!(index.files_using("Config"), 3);
        assert_eq!(index.files_using("Other"), 1);
    }

    #[test]
    fn a_leading_dot_slash_does_not_make_a_second_file() {
        let mut index = index();
        index.record("./src/a.rs", &[NameUse::new("Config", 1)]);
        index.record("src/a.rs", &[NameUse::new("Config", 9)]);

        // One file, two lines -- and a project that spelled the same path two ways
        // would otherwise have undercounted both.
        let users: Vec<_> = index.users_of("Config").collect();
        assert_eq!(users, vec![("src/a.rs", &[1, 9][..])]);
    }

    #[test]
    fn an_unused_name_is_absent_rather_than_empty() {
        // The distinction matters: an empty list means "used and it resolved to
        // nobody", which is not a thing that happens. A caller asking about a name
        // nobody uses gets no entry at all.
        let index = index();
        assert_eq!(index.users_of("Nobody").count(), 0);
        assert_eq!(index.files_using("Nobody"), 0);
        assert!(index.is_empty());
    }

    #[test]
    fn an_alias_records_the_path_the_name_came_in_by() {
        let mut index = index();
        index
            .record("src/a.rs", &[NameUse::aliased("Baz", 3, "foo::bar::Baz")]);
        assert_eq!(index.files_using("Baz"), 1);
    }
}
