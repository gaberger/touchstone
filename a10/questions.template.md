# A10 — 20 questions

Copy to `a10/questions.md`, replace these with **your** questions, and **commit before running
anything**. The commit is what makes the result evidence rather than an anecdote.

## Rules

- **Write them before you search.** A question written after seeing results is a question
  reverse-engineered from what the index happens to hold.
- **Real questions you actually wanted answered**, not questions designed to show a feature.
  The ones that made you go looking last week are the good ones.
- **Mix the shapes deliberately.** If every question is a keyword lookup, `rg` wins and you
  have learned nothing. If every question is a structured filter, you have rigged it the other
  way. Aim for a spread:

| Shape | Example | Tests |
|---|---|---|
| keyword | "what did we decide about post-filtering?" | plain recall — grep should win these |
| synonym | "how do we stop the index drifting?" (the note says *derived plane*) | recall failure → vectors (A4) |
| structured | "which decisions are still Proposed?" | **structure failure → the OKF case** |
| temporal | "what is stale — verified before June?" | structure, on `stale_after` |
| provenance | "which claims are measured, not asserted?" | structure, on trust tiers |
| relational | "what depends on the CRDT decision?" | graph expansion |

If you cannot write 20, that is itself a finding: it means the corpus is not one you actually
query, and A3 — will anyone use this — matters more than A10 right now.

---

1. 
2. 
3. 
4. 
5. 
6. 
7. 
8. 
9. 
10. 
11. 
12. 
13. 
14. 
15. 
16. 
17. 
18. 
19. 
20. 
