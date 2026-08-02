---
type: Runbook
title: "Rebuild the index from scratch"
description: "The T1 drill: delete everything derived, regenerate, diff."
tags: [ops]
status: stable
verified:
  - by: human:gary
    at: 2026-07-15T09:00:00Z
---

1. `rm -rf .touchstone/`
2. `find . -name index.md -delete`
3. `touchstone index`
4. `git diff --exit-code`

Any diff falsifies the derivability claim.
