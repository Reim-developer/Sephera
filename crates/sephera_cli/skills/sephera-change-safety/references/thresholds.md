# Thresholds: what the numbers mean, and the defaults

Every number in this skill is a default a team may tune, not
a law. What is not tunable is what each number counts — that
is the measurement, and changing the meaning silently is how
a gate stops meaning anything.

## The edit threshold: 25 dependents

- **Default:** 25 direct users. This is the number in the
  `guard` docs example, and the skill adopts it as the
  default.
- **What it counts:** for `impact` without `--symbol`, the
  files that depend on the target file. With `--symbol`,
  the files that name the symbol. Namespace importers are
  never counted — they may use the symbol without naming
  it, and counting them would turn every widely-imported
  namespace into a stop.
- **What reaching it means:** stop and get a human's
  agreement before touching the file. It does not mean
  "edit carefully"; it means the edit is not yours to make
  alone yet.
- **How to tune it:** a team that finds 25 too twitchy or
  too lax changes the number in one place — the `guard`
  invocation, the `--fail-on` value, or the skill's local
  copy — and says so where the next reader will see it. A
  threshold tuned in silence is a threshold nobody trusts.

## The breaking gate: broken symbols with consumers

- **What it counts:** broken symbols that still have at
  least one direct consumer in the newer tree. The CLI's
  `--fail-on` compares exactly this count.
- **What it does not count:** a break nobody uses. That is
  the clean kind of breaking, and a gate that fired on it
  would train a pipeline to ignore the gate.
- **No default number.** The skill deliberately sets none:
  the right limit depends on the repo's tolerance for
  breaking changes, and inventing one would be false
  precision. Agree it per repo, write it down, then gate
  on it.

## The coverage gate: untested symbols

- **What it counts:** declared symbols exercised by nothing
  through the import graph. Direct and indirect exercise
  both count as tested; the report keeps the tiers apart
  because they mean different things.
- **No default number**, for the same reason as the
  breaking gate. A new symbol with no test reaching it is
  a fact the merge decision carries; whether it blocks the
  merge is the repo's call.

## The risk score: dependents × churn × (1 + coupled)

- `sephera risk` scores a file as `dependents × churn ×
  (1 + coupled_partners)` and prints every factor, so the
  score is never a black box. It is an investigation, not a
  gate in this workflow: a high score says "look here
  first", not "stop".

## The coupling defaults: window 500, min 3, max 50

- `sephera coupling` mines git history with a window of 500
  commits, a minimum of 3 co-changes for a pair to be
  reported, and commits touching more than 50 files
  excluded (a release commit that touches everything says
  nothing about any pair).
- `containment = co_changes / min(first_commits,
  second_commits)` — how much of the rarer file's history
  the pair accounts for. A pair that co-changes 3 times out
  of 3 commits of the rarer file is a stronger signal than
  3 out of 300.
