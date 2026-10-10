# sephera_gui

`sephera-gui` is the graphical front-end. It does one thing: count lines of code
in a directory you pick, and show them by language.

Every number comes from `sephera_scan`, the same crate `sephera loc` uses, and
every configuration decision comes from `sephera_runtime` — so a repository with
a `.sephera.toml` and a `.gitignore` gets the same count from the GUI as from the
terminal. The GUI adds no counting of its own.

## Running it

```bash
cargo run -p sephera_gui --profile release-gui
```

Pick a directory, or type one and press **Count**. Enter in the directory field
starts a count too. `Ignore` takes extra comma-separated patterns, merged after
`.sephera.toml`'s, exactly as the CLI's flags are.

## What the window shows

One row per language, heaviest first, in the CLI's column order — `Files`,
`Code`, `Comment`, `Empty`, `Bytes` — plus a totals row and a status bar naming
the file count, the elapsed time, and the config file it read if there was one.
That last line is deliberate: a count that changed because of a file the user did
not open has to say so on screen.

## Where the code lives

| path | what |
|---|---|
| `src/lib.rs` | `count`, `loc_view`, `summary` — unit-tested, no `egui` in sight |
| `src/app.rs` | the window, panels, table, spinner |
| `src/main.rs` | `main`, which only picks the starting directory |

The split is not tidiness. An `egui::App::update` needs a window, a graphics
backend and a running event loop, so anything put in one is untestable by
construction. `count` is a pure function of a path, and it is where every case
the GUI's behaviour rests on is pinned — including that a `.sephera.toml`
pattern changes the count and is reported, which is the claim the whole design
rests on.

## Building it for release

```bash
# The CLI, unchanged: fat LTO, `codegen-units = 1`.
cargo build --release -p sephera

# The GUI, on its own profile. Thin LTO and sixteen codegen units, because a GUI
# is idle in its event loop and fat LTO over `winit` and a graphics backend adds
# minutes to every release build to optimise code that is waiting for a click.
cargo build -p sephera_gui --profile release-gui
```

This crate is `publish = false`. It is a binary, and publishing it would pull
`winit`, a graphics backend and a font stack into trees that wanted a line
count. It is also why its `rust-version` is its own rather than the workspace's
`1.85`: `eframe` 0.36 declares `1.95`, and a manifest that claims otherwise is
promising something it cannot keep.
