---
type: Attested Computation
title: Revenue by year
runtime: bigquery
parameters:
  - { name: year, type: integer, required: true }
computation: references/computations/revenue.sql
executor:
  resource: references/skills/run-on-bq.md
  receipt: [job_id, executed_sql, result]
---

Sanctioned execution logic.
