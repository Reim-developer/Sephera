//! A table described once and written by either of two formats.
//!
//! Every count in this tool is a row in a table, and until now each table was
//! spelled out again in each format: `print_symbol_report` built a comfy-table,
//! `render_symbol_markdown` wrote pipe characters, and `symbols_by_file` was
//! aggregated inside the first of those so the other two had to reach back into
//! the analyzer. Nineteen `writeln!` calls in one file is the symptom; the cause
//! is that the *shape* of a report was a property of the renderer rather than of
//! the data.
//!
//! A [`Grid`] holds that shape: columns with alignments, rows, and an optional
//! totals row. [`Format`] knows how to write one. Adding a column is one edit in
//! one place and every format gains it, which is the property that
//! `--by-file` lacked until the e2e tests found it being silently ignored by two
//! thirds of the formats.
//!
//! A worked example lives in the tests below rather than here. This module is
//! private to the binary crate, so a doctest could not name it, and an example
//! that cannot be compiled is not an example.

use std::borrow::Cow;
use std::fmt::Display;

use comfy_table::{
    Cell, CellAlignment, Color, Table, modifiers::UTF8_ROUND_CORNERS,
    presets::UTF8_FULL_CONDENSED,
};

/// Where a column's contents sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// Text, paths, names.
    Left,
    /// Numbers, so digits line up and a magnitude can be compared down a column.
    Right,
}

/// One column: its heading and how its cells align.
///
/// `Copy` because a column is read by every row and every format; making a
/// renderer clone a heading to look at it would be noise at each of the dozen
/// places that inspect one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    heading: &'static str,
    alignment: Alignment,
}

impl Column {
    /// A column of labels.
    #[must_use]
    pub const fn text(heading: &'static str) -> Self {
        Self {
            heading,
            alignment: Alignment::Left,
        }
    }

    /// A column of numbers.
    #[must_use]
    pub const fn count(heading: &'static str) -> Self {
        Self {
            heading,
            alignment: Alignment::Right,
        }
    }

    /// The column heading.
    #[must_use]
    pub const fn heading(&self) -> &'static str {
        self.heading
    }

    /// How this column's cells align.
    #[must_use]
    pub const fn alignment(&self) -> Alignment {
        self.alignment
    }
}

/// A table's contents, in render order.
///
/// Holds strings rather than numbers because a cell may be a path, a label, or a
/// count, and the renderers cannot tell them apart afterwards. The alignment on
/// each [`Column`] is what carries "this is a number" to the point where it
/// matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
    totals: Option<Vec<String>>,
}

impl Grid {
    /// An empty grid with the given columns.
    #[must_use]
    pub const fn new(columns: Vec<Column>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
            totals: None,
        }
    }

    /// Append a row.
    ///
    /// A row whose length differs from the column count is padded or truncated
    /// rather than panicking: a table that renders with a ragged row is a
    /// cosmetic bug, and one that aborts the process over it is a worse one.
    pub fn push(&mut self, row: Vec<String>) {
        self.rows.push(self.fit(row));
    }

    /// Set the totals row, rendered emphasised by every format.
    pub fn set_totals(&mut self, row: Vec<String>) {
        self.totals = Some(self.fit(row));
    }

    /// The columns.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The body rows, in order.
    #[must_use]
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// The totals row, when one was set.
    #[must_use]
    pub fn totals(&self) -> Option<&[String]> {
        self.totals.as_deref()
    }

    /// Whether the grid has no body rows and no totals row.
    ///
    /// A formatter asks this before emitting a heading, because a section whose
    /// table is empty reads as a finding of nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.totals.is_none()
    }

    fn fit(&self, mut row: Vec<String>) -> Vec<String> {
        row.resize(self.columns.len(), String::new());
        row
    }
}

/// Build a grid from an iterable of rows, so callers write a `map` rather than a
/// `for` loop and a `push`.
pub fn grid_of<I>(columns: Vec<Column>, rows: I) -> Grid
where
    I: IntoIterator<Item = Vec<String>>,
{
    let mut grid = Grid::new(columns);
    for row in rows {
        grid.push(row);
    }
    grid
}

/// The output formats a [`Grid`] can be written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// A bordered terminal table.
    Table,
    /// GitHub-flavoured Markdown pipe table.
    Markdown,
}

impl Format {
    /// Render the grid in this format.
    #[must_use]
    pub fn render(self, grid: &Grid) -> String {
        match self {
            Self::Table => render_table(grid),
            Self::Markdown => render_markdown(grid),
        }
    }
}

/// Turn anything displayable into a cell.
pub fn cell<T: Display>(value: T) -> String {
    value.to_string()
}

/// Turn anything displayable into a cell wrapped in backticks, for a Markdown
/// code span.
///
/// Applied by the caller rather than inferred, because whether a path is code is
/// a reading decision and a reader who cannot see the path boundaries will
/// misparse a filename containing a colon.
pub fn code<T: Display>(value: T) -> String {
    format!("`{value}`")
}

/// Escape the one character that would end a Markdown table cell.
///
/// Applies to every cell rather than only to paths, because a renderer cannot
/// know which column holds a filesystem path and a value with a pipe in it is
/// wrong in any column.
fn escape_pipe(value: &str) -> Cow<'_, str> {
    if !value.contains('|') {
        return Cow::Borrowed(value);
    }
    Cow::Owned(value.replace('|', "\\|"))
}

/// One cell of a terminal table, aligned by its column.
///
/// Right-aligning a number column is what lets a reader compare magnitudes down
/// the column instead of counting digits. A missing column falls back to left,
/// which is what a padded row wants.
fn table_cell(column: Option<&Column>, value: &str) -> Cell {
    let cell = Cell::new(value);
    match column.map(Column::alignment) {
        Some(Alignment::Right) => cell.set_alignment(CellAlignment::Right),
        _ => cell,
    }
}

fn render_table(grid: &Grid) -> String {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(
        grid.columns()
            .iter()
            .map(|column| table_cell(Some(column), column.heading()))
            .collect::<Vec<_>>(),
    );

    for row in grid.rows() {
        table.add_row(
            row.iter()
                .enumerate()
                .map(|(index, value)| {
                    table_cell(grid.columns().get(index), value)
                })
                .collect::<Vec<_>>(),
        );
    }

    // The totals row is tinted rather than bold: a terminal table has no markup,
    // and colour is the one emphasis every terminal renders.
    if let Some(totals) = grid.totals() {
        table.add_row(
            totals
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    table_cell(grid.columns().get(index), value)
                        .fg(Color::Green)
                })
                .collect::<Vec<_>>(),
        );
    }

    table.to_string()
}

fn render_markdown(grid: &Grid) -> String {
    let mut out = String::new();

    let separator = |column: &Column| match column.alignment() {
        Alignment::Left => "---",
        Alignment::Right => "---:",
    };

    // `| ` on the left and ` |` on the right of every cell. The asymmetry is
    // GitHub's own convention and matters: without the leading space a cell
    // beginning with `**` would be read as a continuation of the previous
    // pipe rather than as emphasis.
    let row = |cells: &[String], decorate: Option<&dyn Fn(&str) -> String>| {
        let mut line = String::new();
        line.push('|');
        for index in 0..grid.columns().len() {
            line.push(' ');
            // A literal `|` ends the cell, including inside a code span, so a
            // file named `flags|name.rs` would split one path into two columns
            // and shift every column after it. Escaping happens before
            // decoration so the escape applies to whatever ends up rendered.
            //
            // Reachable only on a filesystem that permits it in a filename,
            // which rules out Windows and leaves Linux and git: the name is
            // legal there and a repository can hold one.
            let value = cells
                .get(index)
                .map_or(Cow::Borrowed(""), |value| escape_pipe(value));
            match decorate {
                None => line.push_str(&value),
                Some(wrap) => line.push_str(&wrap(&value)),
            }
            line.push_str(" |");
        }
        line.push('\n');
        line
    };

    let headings: Vec<String> = grid
        .columns()
        .iter()
        .map(|column| cell(column.heading()))
        .collect();
    out.push_str(&row(&headings, None));

    let rules: Vec<String> = grid
        .columns()
        .iter()
        .map(|column| cell(separator(column)))
        .collect();
    out.push_str(&row(&rules, None));

    // Pads to the column count so a short row cannot produce a pipe table with a
    // missing cell, which renders as a column shift rather than as a gap.
    for cells in grid.rows() {
        out.push_str(&row(cells, None));
    }

    if let Some(totals) = grid.totals() {
        out.push_str(&row(totals, Some(&|value| format!("**{value}**"))));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{Alignment, Column, Format, Grid, code, grid_of};

    fn sample() -> Grid {
        let mut grid = Grid::new(vec![
            Column::text("Language"),
            Column::count("Files"),
            Column::count("Total"),
        ]);
        grid.push(vec!["Rust".into(), "2".into(), "7".into()]);
        grid.push(vec!["Python".into(), "1".into(), "3".into()]);
        // The totals row carries no markup of its own: the format decides how to
        // emphasise it, which is the whole reason the row is marked rather than
        // pre-decorated.
        grid.set_totals(vec!["Totals".into(), "3".into(), "10".into()]);
        grid
    }

    #[test]
    fn every_format_has_the_same_column_headings_in_the_same_order() {
        let grid = sample();
        let expected: Vec<&str> =
            grid.columns().iter().map(Column::heading).collect();

        assert_eq!(expected, vec!["Language", "Files", "Total"]);

        for format in [Format::Table, Format::Markdown] {
            let rendered = format.render(&grid);
            let mut previous = 0;
            for heading in expected.iter().copied() {
                let at = rendered[previous..]
                    .find(heading)
                    .unwrap_or_else(|| panic!("{format:?} lost `{heading}`"));
                previous += at + heading.len();
            }
        }
    }

    #[test]
    fn markdown_alignment_follows_the_column() {
        let rendered = Format::Markdown.render(&sample());

        assert!(
            rendered.contains("| --- | ---: | ---: |"),
            "a left column gets `---` and a numeric one gets `---:`: {rendered}"
        );
    }

    #[test]
    fn the_totals_row_is_emphasised_and_present_in_every_format() {
        let grid = sample();

        let markdown = Format::Markdown.render(&grid);
        assert!(
            markdown.contains("| **Totals** | **3** | **10** |"),
            "{markdown}"
        );

        let table = Format::Table.render(&grid);
        assert!(table.contains("Totals"), "{table}");
    }

    #[test]
    fn every_row_carries_every_value() {
        let grid = sample();
        let markdown = Format::Markdown.render(&grid);

        for value in ["Rust", "2", "7", "Python", "1", "3", "Totals", "10"] {
            assert!(markdown.contains(value), "`{value}` missing: {markdown}");
        }
    }

    #[test]
    fn a_short_row_is_padded_rather_than_dropping_the_table() {
        let mut grid = Grid::new(vec![
            Column::text("A"),
            Column::count("B"),
            Column::count("C"),
        ]);
        grid.push(vec!["only".into()]);

        assert_eq!(grid.rows()[0].len(), 3, "padded to the column count");
        assert_eq!(grid.rows()[0][1], "", "the gap is empty, not missing");
    }

    #[test]
    fn an_empty_grid_still_renders_its_headings() {
        // An empty repository is a real answer. A formatter that emits nothing
        // for it makes "no declarations" and "the run failed" look the same.
        let grid =
            Grid::new(vec![Column::text("Language"), Column::count("Files")]);
        assert!(grid.is_empty());

        let markdown = Format::Markdown.render(&grid);
        assert!(markdown.contains("| Language | Files |"), "{markdown}");

        assert!(Format::Table.render(&grid).contains("Language"));
    }

    #[test]
    fn the_grid_builder_and_the_loop_produce_the_same_thing() {
        let rows = [("Rust", 2u64), ("Python", 1)];

        let built = grid_of(
            vec![Column::text("Language"), Column::count("Files")],
            rows.map(|(language, files)| {
                vec![language.into(), files.to_string()]
            }),
        );

        let mut looped =
            Grid::new(vec![Column::text("Language"), Column::count("Files")]);
        for (language, files) in rows {
            looped.push(vec![language.into(), files.to_string()]);
        }

        assert_eq!(built, looped);
    }

    #[test]
    fn alignment_survives_into_the_rendered_table() {
        let grid = sample();

        assert_eq!(grid.columns()[0].alignment(), Alignment::Left);
        assert_eq!(grid.columns()[1].alignment(), Alignment::Right);
    }

    #[test]
    fn a_pipe_in_a_value_does_not_split_the_cell() {
        // A pipe ends a Markdown table cell even inside a code span, so a file
        // named `flags|name.rs` would split one path into two columns and shift
        // everything after it. Legal on Linux and in git; Windows will not let
        // such a file be created, so this is pinned here rather than in the
        // end-to-end tests, which cannot build the fixture on this platform.
        let mut grid =
            Grid::new(vec![Column::text("File"), Column::count("Functions")]);
        grid.push(vec![code("src/flags|name.rs"), "2".into()]);
        grid.set_totals(vec!["Totals".into(), "2".into()]);

        let markdown = Format::Markdown.render(&grid);

        // Three pipes delimit this row: the start, the one separator, and the end.
        // A fourth would mean the value split the cell. The escaped pipe is
        // counted too, so the check is on unescaped separators only -- which is
        // the whole question, since an escaped one renders as a character and an
        // unescaped one ends the cell.
        let data_line = markdown
            .lines()
            .find(|line| line.contains("flags"))
            .expect("the data row");
        assert_eq!(
            data_line
                .split('|')
                .filter(|piece| !piece.ends_with('\\'))
                .count(),
            4,
            "{data_line}"
        );
        assert!(data_line.contains("flags\\|name.rs"), "{data_line}");

        // And the decoration must not undo it: the totals row goes through the
        // same path, so a value carrying a pipe stays escaped after the `**`
        // wrapping.
        let bolded = Format::Markdown.render(&{
            let mut grid = Grid::new(vec![Column::text("File")]);
            grid.set_totals(vec![code("a|b")]);
            grid
        });
        assert!(bolded.contains("| **`a\\|b`** |"), "{bolded}");
    }

    #[test]
    fn a_value_with_no_pipe_is_not_altered() {
        // The escaping must not add a backslash where there is nothing to
        // escape: `a\b` is a Windows path and doubling its separator would
        // corrupt it.
        let mut grid = Grid::new(vec![Column::text("File")]);
        grid.push(vec![code(r"src\windows\path.rs")]);

        let markdown = Format::Markdown.render(&grid);
        assert!(markdown.contains(r"`src\windows\path.rs`"), "{markdown}");
    }
}
