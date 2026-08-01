---
type: Decision
title: "Post-filter authorization at depth"
description: "Retrieve at K>=500 and post-filter rather than building a query planner."
tags: [security, search]
status: stable
verified:
  - by: human:gary
    at: 2026-07-15T09:00:00Z
---

Simulation showed recall recovers fully at K=500.
Does not address revocation -- see [revocation](/terms/revocation.md).
