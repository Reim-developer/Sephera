---
title: guard
description: Judge one agent edit before it happens, from a hook payload on stdin.
---

# `guard`

The pre-edit seatbelt for coding agents.

Agent harnesses such as Claude Code run a hook before every file edit and hand
it a JSON description of the tool call on stdin. `guard` is that hook: it finds
the file being edited, measures its blast radius and affected tests, prints the
numbers on stderr, and answers on stdout in the harness's own decision shape.

```bash
sephera guard --path . --max-dependents 25
```

Judging an edit to `crates/sephera_graph/src/blast_radius.rs` in this
repository, with the limit set to 4 for the sake of the example:

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"`crates/sephera_graph/src/blast_radius.rs` has 4 dependents, at or over the limit of 4. Measure it with `sephera impact crates/sephera_graph/src/blast_radius.rs` and get a person's agreement before editing it."}}
```

A denied edit also exits **2**, so a harness that honours only exit codes and
one that only reads JSON both stop the edit. Without `--max-dependents` every
edit is allowed and the guard is a report only.

## Usage

```bash
sephera guard --file crates/sephera_graph/src/blast_radius.rs --path . --max-dependents 50  # judge one file by hand
echo '{"tool_input":{"file_path":"crates/sephera_graph/src/blast_radius.rs"}}' | sephera guard --path . --max-dependents 50  # the same judgement, from a hook payload
```

The first form skips stdin; the second is what a harness sends. Both allow here, because the file's dependents stay under the limit. Set the limit at or below the dependent count and the same commands deny with exit 2 instead. Without `--max-dependents` every edit is allowed and the guard is a report only.

## Wiring it up

As a Claude Code `PreToolUse` hook for the edit tools:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [
          { "type": "command", "command": "sephera guard --path . --max-dependents 25" }
        ]
      }
    ]
  }
}
```

The payload's `tool_input.file_path` (or `notebook_path`, or `path`) names the
file. Absolute paths are made relative to the project before matching, and the
file does not have to exist yet — a write tool creating it is judged by where
it will live.

## What the guard allows

Every outcome that is not a denial is an allow, including the awkward ones:

- **No edited file in the payload** — a shell command, malformed JSON. Allowed,
  with the reason on stderr.
- **The file is not in the analysed graph** — a new file, a file outside the
  project. Allowed: a file nothing imports yet has no blast radius to judge.

This is deliberate. A guard that blocked edits it could not judge would block
them all the first time it was misconfigured, and be removed by lunch.

## Trying it by hand

`--file` skips stdin entirely, which is how a pre-commit hook — or a curious
person — runs the same judgement a harness would get:

```bash
sephera guard --file src/lib.rs --path . --max-dependents 25
echo $?   # 0 = allowed, 2 = denied
```

## Exit codes

| code | meaning |
|---|---|
| 0 | the edit is allowed |
| 1 | the guard could not run (unreadable stdin, unbuildable graph) |
| 2 | the edit is denied: the blast radius reached `--max-dependents` |

## Related commands

[`impact`](/commands/impact/) is the measurement the guard enforces, and
[`affected-tests`](/commands/affected-tests/) is the follow-up it reports.
An agent with Sephera's MCP server configured can ask for both itself — the
guard exists for the moment before it thinks to ask.

## Haiku

> Judge the edit first
> measure the blast radius
> allow or deny
