# _sample — a corpus that looks like someone's actual desk

Test material for the ingest pipeline. Deliberately **not** a bundle: it is the pile you point
`touchstone ingest` at.

```bash
touchstone --bundle ~/scratch init
touchstone --bundle ~/scratch ingest _sample
touchstone --bundle ~/scratch unprocessed --content --limit 1
```

| | |
|---|---|
| `notes/` | markdown, the kind actually kept — a standup, a reading note, a one-line thought |
| `docs/` | a PDF, an interview transcript, an email thread |
| `images/` | a real PNG and a real JPEG, with valid headers |
| `video/` | an MP4 with a genuine ISO container header |
| `data/` | CSV and JSON |

Real bytes throughout — `file(1)` identifies every one. A corpus of `.md` files pretending to
be images would not exercise the property that matters, which is that ingest and export leave
binaries untouched.

The content is a small connected story rather than lorem ipsum: a timestamp bug in a finance
report, an interview that explains it, an email confirming the scope, and the data to check it
against. That is what makes it useful for retrieval questions — "which timestamp does the
finance report use?" has an answer spread across three documents, which is the shape real
questions have and synthetic corpora never do.
