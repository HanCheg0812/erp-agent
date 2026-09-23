---
name: erp-fact-check
description: Verify current order and supply facts through authorized ERP tools before making operational claims or comparing a change.
allowed-tools: [mcp__erp__read_erp_order]
---

Read the identified order and relevant line within the case scope. Preserve the returned state version, retrieval time, quantity basis and unit. Do not equate ordered quantity with remaining demand or on-hand stock with allocatable stock.

Keep ERP facts separate from historical documents and saved proposals. If required supply or capacity tools are absent, state that those checks remain incomplete.

Tool reads do not approve changes. Before a later write, backend approval and current-state validation must still succeed. A disconnected write requires reconciliation before any retry; an approval or sandbox snapshot is not proof of ERP execution.
