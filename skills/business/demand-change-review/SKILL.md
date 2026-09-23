---
name: demand-change-review
description: Review manufacturing demand changes involving quantities, delivery dates or revisions using the authorized case and available evidence.
allowed-tools: [case_read, todo]
---

Read the current case before comparing a request with its saved proposal. Identify whether each quantity is incremental, cumulative or a replacement total. If the basis is unknown or conflicting, state what needs confirmation instead of calculating a delta as fact.

Keep customer, part, revision and effectivity together. Distinguish a customer's requested date from a feasible production or delivery commitment. Saved proposal values are a comparison baseline, not a fresh ERP read.

For a substantial review, track analysis with todo. Separate supported changes, missing evidence and suggested next actions. A recommendation remains a draft until the backend binds approval to an exact proposal and action list.

Use ERP and knowledge tools only when actually offered by the runtime. When they are unavailable, explain the specific verification still needed. Do not infer stock, capacity, document contents or successful ERP writes from a saved case or approval.
