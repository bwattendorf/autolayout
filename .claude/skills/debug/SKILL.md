---
name: debug
description: Systematic debugging workflow (reproduce, probe, examine, fix) for autolayout. Use when a board will not route to 100%, a result is worse than expected, a change "should have helped" and did not, or any bug resists the obvious approaches. Based on Kuhlmann, The Book of Debugging (No Starch Press); chapter numbers below refer to it.
---

# Debugging workflow

Use this when something fails and the cause is not obvious. The book's core claim: the computer is
deterministic, so every bug has a cause that can be found. A workflow keeps effort from scattering.
Loop through the four steps until the bug cannot be reproduced; always start with Reproduce.

## 0. Before anything else

- Take responsibility: assume the bug is in our code, not in KiCad, the corpus, or "the board" (Intro).
- Open or create a debugging log for this bug (see Log below) and write down the symptom, what you
  expected, and the exact command that shows it. Symptoms only; keep theories out of that line (Part I).
- Do one bug at a time, and one board at a time. Finish or park it before starting another (ch. 18).

## 1. Reproduce (ch. 1-3)

- Get the failure to happen on demand, from a named input and a command line. A result nobody can
  re-run is not a result (CLAUDE.md rules 3, 4, 7).
- **Shorten the loop first.** If one run takes minutes, make a faster path before investigating
  (autolayout: `route --quick --iters N` runs one attempt in about 40 s on c4 instead of 6-10 min).
  Fast loops are worth more than clever theories.
- **Cut the case down** (ch. 20, minimal repro): crop to the smallest board, region or net set that
  still fails. If it stops failing, the part you removed matters; put it back in halves.
- Vary one thing at a time to learn which steps matter: order, timing, seed, layer count, window size.
- Automate the repeat. Keep the repro as a script or a test once the bug is fixed (ch. 3, 25).

## 2. Probe: find an entry point (ch. 4-9)

- Make an educated guess, but cap the time: stop after about an hour, or after two or three shortcuts
  that did not pan out, or when the guesses stop being based on evidence (ch. 4). Switch strategy then.
- Rule systems out as well as in. Check the code that has broken before, and the glue between
  systems, before looking at old trusted code or libraries (ch. 4).
- **Bisect when it used to work** (ch. 6): find a good commit and a bad one and `git bisect` between
  them; build or reuse binaries; then read the offending diff for the one change that matters.
- **Instrument before theorising** (ch. 8): assertions as early as possible, check every return
  value, make bad data look obviously bad. Keep instrumentation optional (env switches, flags), off
  by default. Do not let it change the behaviour you are chasing; re-check the bug still happens.
- **Remove randomness** (ch. 9): fixed seeds, fixed order, no clock. A "random" result has a cause.
  (autolayout's results are deterministic; a result that moves with a trivial change is telling you
  about a sensitive spot, not about luck.)
- Look at the data the program actually produced (maps, dumps, plots) before reasoning about it
  (ch. 15). Examples here: `route --explain`, `AUTOLAYOUT_MAPS=1`, `AUTOLAYOUT_CONTEST=1`.

## 3. Examine: find the root cause (ch. 13-19)

- **Follow the data** (ch. 15): find the original input, follow it through each transformation, find
  where it first becomes bad, compare good and bad cases side by side.
- **Question everything** (ch. 16): list your assumptions and verify each one. Do not trust a feature
  because it exists; check that it actually fires (count how many pads a planner covered, how many
  times a branch ran). A symptom can sit far from its cause.
- **Experiment with a hypothesis** (ch. 17). For each experiment write down: what is the hypothesis,
  how will it be tested, and what will be learned either way. Test with the least effort. Try the
  hackiest possible fix first to confirm you found the right place, then do the clean one.
  Make experiments revertible: mark them, or keep a clean tree so `git diff` shows them.
- **Do not revert a failed experiment as "no good" until it has been re-tested on a baseline known
  to be correct.** If a bug in the baseline is found later (as the pad-extent bug was), re-run the
  reverted experiments.
- **Keep a debugging log** (ch. 18): every idea, every result, ruled-out areas, a prioritised list of
  next leads so there is always a plan. Store it with the project and reference it from the commit.
- **Build tools** (ch. 19): when you keep wanting to see something, make a flag or a command for it
  and document it. A hidden tool does not help.
- Talk it through (ch. 23) when stuck: stating the problem to someone (or in the log) often exposes
  the assumption that is wrong.

## 4. Fix (ch. 23-30)

- A great fix is minimal, complete, revertible, and addresses the root cause, not the symptom.
  One bug per change (ch. 23, 24).
- **Verify** (ch. 23): reproduce several times; undo the change and confirm the bug returns; re-apply
  and confirm it goes. Test the fix alone, without other local edits. For us: KiCad DRC and the
  unconnected count, on the real board, with the number in the commit message.
- Check side effects separately from the fix: run the other boards (one at a time) and compare
  (ch. 24).
- **Share what you learned** (ch. 25): commit message with root cause, symptom and solution; update
  ROADMAP/gaps; ask how the cause got in and what safeguard prevents it (an assert, a test, a check
  in `ci.sh`). Turn the repro into a regression test when practical.
- **Search for related problems** (ch. 26): the same mistake may exist elsewhere (the pad-extent bug
  affected every use of `bbox` on a rectangle). Fix the family while the context is fresh; make bulk
  changes with care.
- If a clean fix is not possible yet: deploy a quick fix or workaround and say so (ch. 28), or rewrite
  the offending piece from scratch and compare the two (ch. 29), or change the requirement (ch. 30).
  Do not stop at the workaround; record what remains.

## Anti-patterns seen in this project (avoid)

- Tuning parameters one at a time hoping a number moves (desperate guesses, ch. 4).
- Reverting an idea on one bad run, and trusting a baseline that had a latent bug.
- Reading a count as proof without looking at why (look at the cells, the pockets, the nets).
- Running a ten-minute experiment when a forty-second one answers the same question.
- Claiming a feature works because it compiled; measure that it covered what you think it did.

## Debugging log template

Create `docs/debug-logs/YYYY-MM-DD-<short-topic>.md`:

```
# <topic>  (board, command, commit)
Symptom (objective, no theory):
Expected:
Repro command + input + seed:   (fast version: ...)
Frequency / measured numbers:

## Leads (prioritised: effort vs chance)
1. ...

## Experiments
- [date] Hypothesis / test / result / what it rules in or out (attach file paths, numbers)

## Ruled out
- ...

## Root cause (when found) / fix / verification / related places checked
```

Every number in the log names its board, command and whether it was measured (CLAUDE.md rule 7).
