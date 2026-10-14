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

## You are in charge of this round

The cloud session writes the round, but once you are running it the decisions
are yours. Specifically:

- Do not hand a decision back that you can make. "I would rather fix X first"
  or "I am deliberately not doing Y" is a decision the round already made if
  the round lists Y. Do Y. If Y has a side effect (a KiCad assertion dialog,
  a long run), run it ONCE, say so in one line first, and carry on; do not run
  it in a loop.
- Every step in a round gets one of three outcomes in the log: done, done
  partly (say which half is missing), or blocked (say by what exact error).
  "Skipped" and "not run" are not outcomes unless the round says to skip.
- When a step shows the plan was wrong (a premise of the round is false, two
  tables disagree), say so at the top of the log, work out the corrected
  command yourself, and run that. Do not stop at the discrepancy.
- Start long jobs (Freerouting, full batches) in the background first, then do
  the short steps while they run.
- A step that finds an unrouted pin or a router weakness is a finding for the
  cloud session, not a reason to stop: log it and go on to the next step.

## Keep going: do not stop between rounds

The cloud session queues several rounds ahead, so there is normally more to do.

1. When a round is done and pushed, `git pull` and look for the next
   "Round N+1" without a log. If there is one, do it now, in this chat.
2. If none, do the first applicable Backlog item. If none applies either, say
   so in one line and stop.
3. A long background job (Freerouting, a 4 h batch) never blocks the next
   round. Start it, record its output path in the log, and go on to the next
   round. Do not sit watching it. Whoever finishes first appends its result
   to the log of the round that started it.
4. Restart in a fresh chat only when the conversation is long (many large tool
   outputs, or you have been compacted). Then commit and push, and end with one
   line: `Done through Round N (<hash>). Start a fresh chat and send: Read
   docs/windows-agent.md and do what it says.`

## Test the change on the boards it targets before a full batch

A full `m7_batch` takes about an hour. When a round tests a change aimed at
named boards or at one mechanism, run just those boards first (`--boards GLOB`
takes several globs) and check the round's premise on them (the log line, the
count, the coordinates it names). Run the full batch only if the premise holds.
If the premise fails, that failure is the result: log it with the evidence and
stop that round there.

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
