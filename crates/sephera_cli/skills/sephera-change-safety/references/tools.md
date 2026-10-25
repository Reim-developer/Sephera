# MCP tool reference for the change-safety workflow

The same workflow as the CLI, through the MCP server's tools.
Each tool mirrors its command's report — same measurement,
same tiers, same suggestion — rendered as JSON with a
Markdown body. The tool names below are the registered names;
the server's tool list is the authority if one ever differs.

## Step 1–2: `impact`

The `impact` tool takes the file path and an optional `symbol`
parameter. The report separates direct users (the count a gate
compares), namespace importers (may-use, never counted), and
downstream files, exactly as the CLI does. There is no
`--fail-on` on the tool: compare the direct-user count
yourself against the threshold and stop the edit when it
reaches it.

## Step 1 (enforced): no `guard` tool

`guard` is a CLI-only hook: it reads a harness payload on
stdin and answers with a decision JSON plus an exit code.
There is no MCP equivalent, because a hook decision is not a
report. When the harness runs a guard, honour its verdict;
when it does not, run `impact` and apply the threshold
yourself.

## Step 3: `review`

The `review` tool takes the base ref (HEAD by default) and
returns changed files with blast radius and affected tests,
the breaking section composed from the same measurement as
the `breaking` tool, and the coupling warnings. Read it in
that order: breaking first, then the changed files, then the
warnings. Like the CLI, it is report-only — the gate lives
at `breaking`.

## Step 5 (gate): `breaking`

The `breaking` tool takes `from` and an optional `to`. Its
`BrokenSymbol` entries carry the same suggestion shape as the
CLI, flattened for agents: an `action` of `moved` or
`new_call_shape`, with `file` and `declaration_kind` for a
move, or `signature` for a new call shape; absent (null)
when the newer tree says nothing actionable. The gate rule
is yours to apply: count the broken symbols with at least
one direct consumer and fail the merge step when that count
reaches the agreed limit.

## Step 5 (coverage): `untested`

The `untested` tool returns the declared symbols no test
reaches, with direct and indirect tiers kept apart, plus a
coverage count the CLI's `--fail-on` compares. Apply the
same rule yourself: a new symbol with no test reaching it
is a fact the merge decision carries, not an automatic fail.

## Investigations, not gates

- `coupling` — the co-changing pairs, same measurement as
  the CLI.
- `risk` — the score with every factor printed.
- `dead` — the unreachable files and symbols.
- `affected_tests`, `graph`, `why`, `symbols`, `context`,
  `loc` — the supporting reads the workflow's reports are
  built from. `why` answers, for one file, which chain of
  imports reaches it; use it when a blast radius surprises
  you and you want the path, not just the count.
