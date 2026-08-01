---
type: Metric
title: Three-entry verification chain
verified:
  - by: human:alice
    at: 2026-01-01T00:00:00Z
  - by: process:nightly
    at: 2026-02-01T00:00:00Z
  - by: human:carol
    at: 2026-03-01T00:00:00Z
sources:
  - id: src-a
    resource: https://example.com/a
    title: Source A
    usage_count: 5000
  - id: src-b
    resource: https://example.com/b
usage_window: { from: 2026-06-01, to: 2026-06-30 }
---

A claim with attribution.[^src-a]
