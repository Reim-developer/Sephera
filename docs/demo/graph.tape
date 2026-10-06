# The reverse dependency query, recorded.
#
# One question, answered in one command: you are about to change `parser.rs`.
# Which files break?
#
# Regenerate with `python scripts/record_demo.py`. The tape lives in the
# repository because the GIF is generated output and this is its source; a
# binary recording nobody can re-make is a binary that quietly goes stale.
#
# The project is `fixture/` rather than this repository, for two reasons. The
# answer fits one screen, where a 1,400-file report scrolls the summary away
# before the reader sees it. And its output is fixed: as this repository grows,
# the recording still shows the same three arrows, forever.

Output docs/public/demo/graph.gif

# `vhs` rejects `powershell.exe` as a shell name on Windows, so the default
# `cmd` is chosen deliberately rather than by omission.
Set Shell "cmd"

Set FontSize 12
Set Width 940
# Sized to the whole 42-line report, so the header and the `**Query:**` line stay
# on screen. Measured, not guessed: at 14pt the top eight lines scrolled off and
# took the question with them.
Set Height 830
Set Padding 20
Set Theme {"name":"Catppuccin Mocha","background":"#1e1e2e","foreground":"#cdd6f4","cursor":"#f5e0dc","selection":"#585b70"}
Set TypingSpeed 50ms

Hide
Type "sephera graph --path docs/demo/fixture --what-depends-on src/parser.rs --format markdown"
Enter
Show

# The analysis. The report is already filtered to the blast radius, so the
# diagram at the end is the answer: three files reaching the one being changed.
Sleep 5s