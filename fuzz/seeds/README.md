# Fuzz seed corpora

Small inputs committed on purpose. A fuzz target's corpus proves the assertion
has teeth only if re-running it against a *reverted* fix still fails — and it only
finds the old inputs if they are still there. Everything else under `seeds/`
(fuzzer working corpora, crash artifacts) is ignored; these two directories are
not.

Each file is the raw input format of its target, split on a NUL byte.

## `resolve_focus_scope`

| seed | what broke |
|---|---|
| `union-with-whole-base` | `--focus . --focus src/core` returned `{src/core}`. Scopes are a union and the base contains `src/core`, so the answer is the whole base. Answering with the narrower scope undercounts dependents, which is how a `--fail-on` limit stops firing without the rule changing. |
| `union-reversed` | The same two scopes in the other order. A set must not depend on which one was typed first. |
| `leading-dot-slash` | `--focus ./crates/x` matched no node, so the blast radius came back as zero with nothing to say why. |
| `trailing-parent` | `--focus a/b/..` resolved nowhere and reported nothing. |
| `walks-back-to-base` | `--focus x/../..` walks back to the base, which is the same scope as `.`. |
| `windows-spelling` | Backslash-separated input. Only a separator where a backslash is one. |

## `graph_path_utils`

| seed | what broke |
|---|---|
| `empty` | The empty path. Not a defect — the point of the seed is that `"".split('/')` yields one empty piece while `segments("")` yields none, which is the difference between a check that holds and one that fires on correct behaviour. |
| `double-separators` | `a//b` and `a/b` must be the same path. |
| `trailing-separator` | A trailing separator, which no node path carries. |
| `only-dot` | `.`, the spelling of "the whole base". |
| `escaping-parent` | `../outside`, which cannot be resolved without knowing what the base is relative to. |
| `backslashes` | A backslash in the path, which separates on Windows and is an ordinary character on Unix. |
| `trailing-dots-in-name` | `a.b.c`, so a dot inside a segment is not mistaken for a `.` segment. |

## Adding one

Write the bytes that a person or a fuzzer would produce, not a normalised form of
them. Every entry above is the exact spelling that failed. Add it here with the
reason, because a seed without a reason is indistinguishable from a seed that
happened to be lying around.

To check a new seed has teeth:

1. revert the fix,
2. run the target against this directory,
3. confirm it fails, and note *which* assertion fired,
4. restore the fix and confirm it passes.

Step 3 is the one that gets skipped, and a target that passes against broken code
is worse than no target — it reads as protection.
