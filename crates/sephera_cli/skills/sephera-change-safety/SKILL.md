---
name: sephera-change-safety
description: "Use Sephera to keep a code change safe: check a file's blast radius before editing it, re-check every file the change touches, review the change before committing, fix breaks by following their suggestion, and gate the merge on breaking symbols that still have consumers. Trigger this skill when editing, reviewing, or committing code in a repository where Sephera is available."
---

# sephera-change-safety

Sephera answers one question, statically and offline: **what breaks
if I touch this?** This skill is the workflow around that question.
It does not replace reading the reports — it says when to run
which check, what number stops the work, and what to do about a
break once one is found.

The commands below are the CLI form. If you are working through
the MCP server instead, the same steps map to tools — see
[tools.md](references/tools.md). Command flags and thresholds
live in [commands.md](references/commands.md) and
[thresholds.md](references/thresholds.md); read those files
when you need an exact flag, not before.

## 1. Before editing a file: check its blast radius

Run `sephera impact` on the file (narrow with `--symbol` when
the change touches one exported name, not the whole file):

```sh
sephera impact <FILE> --path .
sephera impact <FILE> --symbol <NAME> --path .
```

Read the report in three tiers, and never merge them:

- **Direct users** — files that name the file, or the symbol.
  This is the count a gate compares.
- **Namespace importers** — files that import the target's
  namespace wholesale. They *may* use the symbol without naming
  it. They are reported apart and never counted as users.
- **Downstream** — files reached through the direct users.
  They break second, not first.

**Stop rule.** If the direct-user count reaches the threshold
(default 25 — see [thresholds.md](references/thresholds.md)),
stop and get a human's agreement before touching the file.
A wide blast radius is not a reason to edit carefully; it is a
reason not to edit yet. If the harness runs `sephera guard`,
its verdict is this same check, enforced: `deny` at or over
`--max-dependents` means the edit does not happen.

Two errors are information, not noise: a path that matches
nothing in the analysis is an error, not an empty report (an
empty report and a typo are indistinguishable on a screen), and
a tree that is not a git repository errors for the commands
that need history. Do not work around either by guessing.

## 2. While editing: re-check every new file touched

A change that quietly grows its blast radius is the failure
mode this skill exists to catch. Every file the change newly
touches gets the same impact check as step 1, with the same
stop rule. A file added mid-change is not exempt because it
was not in the original plan — it is suspect *because* it
was not in the original plan.

## 3. Before committing: review, breaking section first

```sh
sephera review --path .
sephera review --against <BASE> --path .   # a whole branch, before push
```

Read the report in this order:

1. **The breaking section.** It names, by symbol, what the
   change removed or reshaped, joined to the files that still
   name those symbols. A removed symbol with direct consumers
   blocks the commit until fixed — it is not a warning to
   note and merge anyway.
2. **The changed files and their blast radii**, with the tests
   that reach them.
3. **The coupling warnings.** A file history keeps changing
   with a changed file, but the change does not touch, is a
   file the change probably should have touched. A pair where
   both sides changed is history working as it should and
   does not warn.

`review` is report-only: it has no gate flag. The gate lives
in step 5.

## 4. Fixing a break: follow the suggestion, not the symptom

Every broken symbol carries a suggestion when the newer tree
says something actionable. Follow it literally:

- **Moved** — the name now lives in exactly one other file.
  Update the import to point there. The consumer is still
  broken until the import changes; the suggestion says where
  to point it, not that the break went away.
- **New call shape** — a signature changed. Update the call
  to the shape shown. Do not update the declaration to match
  the old callers; the new shape is the intent.
- **No suggestion** — the name vanished outright, two files
  claim it (suggesting one would send the consumer to the
  wrong file), or the kind changed (the change itself already
  states the new kind). In that case there is nothing safe to
  guess: find the new home or the new contract by reading
  the code, not by picking a candidate.

**Never re-declare a removed name to silence the report.**
That turns a visible break into a silent fork: two
declarations, one name, and the next person to read the
report finds neither.

## 5. Before merging: the gate, and the coverage question

The gate is `breaking --fail-on`, not `review`:

```sh
sephera breaking --from <BASE> --fail-on <N> --path .
```

It exits 2 when at least that many broken symbols still have
direct consumers in the newer tree. A break nobody uses does
not fail the run — that is the clean kind of breaking, and a
gate that fired on it would train a pipeline to ignore the
gate. `--to` names the newer side explicitly; when absent,
the newer side is the working tree, which is the form a
branch review takes before it is pushed.

Then ask the coverage question:

```sh
sephera untested --fail-on <N> --path .
```

It reports the declared symbols no test reaches, measured
statically through the import graph — no test run. A new
symbol with no test reaching it is not a failed gate by
itself, but it is a fact the merge decision should carry:
the change ships code no test exercises. Note the stated
limit: inline tests (a Rust `#[cfg(test)]` module inside the
file it tests) do not cross an import edge, so those symbols
read as untested unless a separate test file names them.

## What this skill is not

- It is not a substitute for the reports. If a number in this
  file and a number in a report disagree, the report wins.
- It does not cover dead code, coupling mining, or risk
  scoring as workflow steps — `dead`, `coupling`, and `risk`
  exist and are documented, but they are investigations, not
  gates in this workflow. Use them when the review raises a
  question they answer, not on every change.
- It does not edit code. Sephera reads; the editor edits.
