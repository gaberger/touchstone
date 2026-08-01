---
type: System
title: "Retrieval pipeline"
description: "Structured prefilter, BM25, graph expansion, trust rank."
tags: [search]
status: stable
generated: { by: capture/claude-opus-5, at: 2026-07-20T12:00:00Z }
---

Over-retrieves to depth 500 because [post-filtering](/decisions/postfilter-is-enough.md)
needs depth to preserve recall.
