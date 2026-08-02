# A3 — will anyone actually write into it?

Listed **project-fatal** in [PROTOTYPE.md](../PROTOTYPE.md), alongside A10. A10 now has a
caveated result (FINDINGS E11); this one is untouched, and is the larger unanswered risk.

> A brain nobody writes to is an empty database with good latency.

## The measurement

Not a survey. **Did you write into it on days nobody asked you to?**

The binary appends one line per capture to `<bundle>/.a3/capture.jsonl` — automatically, from
both surfaces. You supply the one thing it cannot know: which days you were working *on this
project*. That split is the experiment.

```bash
touchstone --bundle ~/brain new Note "..."     # logged, surface=cli
$EDITOR a3/project-days.txt                    # mark project days AS YOU GO
a3/report.sh ~/brain                           # reads out at 21 days
```

## Pre-registered kill criterion

> If concept-creation rate on non-project days trends to zero by week three, editing is not
> easy enough, and no amount of retrieval quality will save it.

`report.sh` applies exactly that: **non-project captures in the final seven days**. It refuses
to give a verdict before 21 days elapsed, and refuses without the day split.

A strong first week followed by silence is the failure this is built to catch, which is why
the criterion is the trend and not the total.

## What is logged, and what is not

| field | why |
|---|---|
| `at` | the day-level series |
| `surface` | `cli` = a human typed it; `mcp` = an agent called it |
| `path`, `type` | what got captured |
| `trust` | always `machine` or `unattributed` — nothing here can write `human` |
| `elapsed_ms` | command wall time |

**`surface` is the field that makes this honest.** An agent writing daily through MCP would
otherwise read as adoption. A3 asks whether a *person* writes; agent captures are reported
separately and never counted.

**`elapsed_ms` is a lower bound, not the number PROTOTYPE.md asks for.** The clock that matters
starts when you decide to record something — reaching for the terminal, finding the window —
and no program can see that. The ~20-second bar has to be judged by hand; this only proves the
tool is not itself the bottleneck.

## Limits worth stating before you start

- **The tool cannot see intent.** "Unprompted" is proxied by the day split, and the proxy is
  imperfect: a non-project day where someone asked you to write something up still counts.
- **Observation changes behaviour.** Knowing captures are counted makes you capture more. That
  biases *toward* adoption, so a failure is strong evidence and a pass is weaker than it looks.
- **Logging never blocks capture.** A failed write is swallowed deliberately. An experiment
  that could stop you recording a thought would corrupt the thing it is measuring.
- **Three weeks is the point.** There is no way to shorten it, and a two-week readout answers
  a different question.
