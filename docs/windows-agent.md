# Standing orders for the Windows session

One prompt starts this session and it never needs to be re-pasted:

> Read docs/windows-agent.md and do what it says.

The cloud session writes each round into `docs/windows-handoff.md`; you write
results into `docs/m7-results/` and push. Nobody copies text between chats.

## The loop

1. `git fetch origin && git checkout m7-router && git pull`. Record the hash.
2. Find the newest "Round N" heading in `docs/windows-handoff.md`. If
   `docs/m7-results/*-roundN.md` already exists, no round is pending: go to the
   "Backlog" section at the end of `docs/windows-handoff.md` and do its first
   item whose condition holds. Only if no backlog item applies, say so in one
   line and stop (or, under `/loop`, wait and check again).
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

## Keep going (adapted from VectorCraft's agent instructions)

VectorCraft's `CLAUDE.md` tells its agents to start every session from the
ROADMAP and the ranked gaps list, run orient -> plan -> implement + test ->
verify -> record -> commit, and "don't stop to ask unless the protocol lists
the decision as the user's". The same applies here:

- Do not stop to ask about anything this file or the round's Steps already
  decide. Pick the obvious reading, note it in the log, and continue.
- Ask only for things that need the user: a missing file you cannot find, a
  tool that needs installing, or anything that would overwrite or delete a
  board.
- Record as you go. Write each step's result to the round log as soon as it
  is done (and commit unfinished work on `m7-router` rather than holding it),
  so a session that stops loses one step, not the round.
- One round, one commit of logs; message `M7.5: round N record`.
- A number in a log names the board, the command, and whether it was measured.

## When to restart

A long chat costs more on every turn, and all state is in the repo, so a fresh
chat loses nothing. After a round is finished, its log pushed, and nothing is
running:

1. End your reply with one line: `Round N done and pushed (<hash>). Start a
   fresh chat and send: Read docs/windows-agent.md and do what it says.`
2. Then stop. Do not start the next round in the same chat.

Do the same, mid-round, if the conversation has grown long (many large tool
outputs, or you have been compacted): commit and push what is done so far
with a note of the step you were on, say so in the same one line, and stop.
Never restart while a batch or Freerouting run is still going; wait for it,
write the log, push, then say it.

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
