# Standing orders for the Windows session

One prompt starts this session and it never needs to be re-pasted:

> Read docs/windows-agent.md and do what it says.

The cloud session writes each round into `docs/windows-handoff.md`; you write
results into `docs/m7-results/` and push. Nobody copies text between chats.

## The loop

1. `git fetch origin && git checkout m7-router && git pull`. Record the hash.
2. Find the newest "Round N" heading in `docs/windows-handoff.md`. If
   `docs/m7-results/*-roundN.md` already exists, nothing is pending: say so in
   one line and stop (or, under `/loop`, wait and check again).
3. Do that round's Steps exactly, with its "Prompt to paste" rules (no
   autolayout code changes; fresh output dirs; never overwrite a board; no
   PR; nothing over 1 MB committed).
4. Write `docs/m7-results/YYYY-MM-DD-roundN.md` as its Log section says.
   Commit, `git push origin m7-router`.
5. Reply with at most ten lines: the hash, the headline totals, anything that
   got worse. The full detail is in the log file; do not paste it into chat.

If a step cannot run (missing netlist, Java, a tool breaks), do the others, make
the smallest fix to a script in `tools/` if that is what broke, and say what
was skipped in the log. A half-answer is recorded as a half-answer.

## Keeping the token use down

- Run batches in the background, redirect their output to files, and read only
  the summary files (`summary.md`, `results.csv`); never print a whole DRC
  report or a per-board log into the conversation.
- Before reading a file, search it for what you need (`grep`, `head`) rather
  than opening it whole. Do not re-read files you already read this session.
- Use the existing scripts (`tools/m7_batch.py`, `m7_check.py`,
  `m7_place_compare.py`, `route_check.py`); do not rewrite them.
- Compare rounds with `results.csv` diffs, not by re-reading old logs.
- Do not summarise the repository or the task back; just do the next step.
- When a long run is going, stop talking until it finishes.
