---
title: skill
description: Install the sephera-change-safety agent skill into a project, or show where it would go.
---

# `skill`

Agents are the fastest-growing users of dev tools, and they do
not read READMEs. Sephera's MCP tools give an agent hands;
this skill gives it judgment: the workflow that says when to
check a change, what threshold stops an edit, and how to fix
a break from its suggestion.

The skill lives in the repo at
`skills/sephera-change-safety/`, and `skill` is how it reaches
a project. The workflow itself is the skill's body — this page
is only the install story.

## Install

```bash
sephera skill install
sephera skill install --dir .agents/skills
```

The target is `<skills-dir>/sephera-change-safety`, where the
skills directory is the `--dir` override or `.claude/skills`
inside the project. A relative override resolves against the
project directory, which is the current directory unless
`--path` names another project.

An existing skill is left untouched when install runs again.
That is deliberate: the target may hold a skill the user tuned
by hand, and overwriting it without being asked would destroy
work this tool cannot reconstruct. To replace it, run the
install again with the overwrite flag:

```bash
sephera skill install --force
```

## Print instead of installing

```bash
sephera skill --print
```

This prints the install path and a one-line summary instead of
copying anything, for agents and scripts that prefer to do it
themselves. The `--print` form is the read-only half of the
command: with no subcommand, `skill` prints the same summary. The print form cannot be combined with install: one
says "copy nothing" and the other says "copy", and picking
silently would make one of the two spellings a lie.

## What the skill says

The workflow, in brief — the skill's own body is the authority:

1. Before editing a file, check its blast radius and stop at
   the threshold when it is reached.
2. Re-check every file the change newly touches.
3. Before committing, review the change and read the breaking
   section first.
4. Fix a break by its suggestion, never by re-declaring the
   removed name.
5. Before merging, gate on breaking symbols that still have
   consumers, and ask what no test reaches.
