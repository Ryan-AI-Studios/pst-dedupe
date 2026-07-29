# 0084-NamedPropCloudAttach — Review

**Status:** Completed (engineering + final Codex PASS)
**Date:** 2026-07-29
**Branch:** `feat/0084-named-prop-cloud-attach`
**Ledger:** FEATURE `50332b73-20a2-4747-8a6c-c1bc4b90498f`

## Objective shipped

Reader-side MS-PST NPMAP (`0x61`) resolve + attachment-table cloud/modern detect; `ATTACH_CLOUD_LINK` ledger with `cloud_provider`/`cloud_url`; Mode A incomplete; unique-PST pointer preserve (anti-ghost); no hydration; closes **D-0080-cloud-attachments** (attach-table detect only).

## Review loop

| Round | Reviewer | Verdict | Notes |
|---|---|---|---|
| Internal #1 | subagent | PASS WITH DEFERRED P3 | Missing Mode A×CloudLink test |
| Fix | orchestrator | — | Added `mode_a_promotes_physical_peer_over_cloud_link` |
| Internal #2 | explore | PASS | P3 closed |
| Codex #1 | gpt-5.6-luna high | FAIL | P1 GUID index; P2 e2e/doc/filename |
| Fix | subagent | — | Protocol wGuid 1/2/n-3; docs; empty name; e2e |
| Codex #2 | gpt-5.6-luna high | FAIL | P2 classic ref over-class; P2 BY_VALUE method |
| Fix | orchestrator | — | Method-only = web-ref 7; force method 7 for CloudLink by-value |
| Codex #3 final | gpt-5.6-luna high | **PASS** | No findings |

## Design (review fold-in)

- Explicit `is_cloud_link` (not only `stream_available`)
- Independent OR: named provider; web-ref method 7; URL-shaped path fallback
- Classic ref methods 2/3/4 stay METHOD_UNSUPPORTED omit unless named/URL signal
- NPMAP wGuid: 0=none, 1=PS_MAPI, 2=PS_PUBLIC_STRINGS, ≥3 stream[n-3]
- Writer CloudLink pointer row: no binary; method honesty (no BY_VALUE without payload)
- CSV append-only `cloud_provider,cloud_url` + formula neutralization

## Deferred

| ID | Disposition |
|---|---|
| D-0080-cloud-attachments | **Closed** (attach-table detect) |
| D-0068-04 | Narrowed (reader allowlist; full set residual) |
| D-0084-body-cloud-links | **Open** — body-inline URLs |
| D-0084-cloud-named-prop-write | **Open** — full NPMAP/named-prop re-emit |

## Gates (orchestrator)

- cargo fmt --all --check
- cargo clippy affected crates -D warnings
- cargo test -p pst-reader / dedup-engine / pst-writer / pst-dedup-cli (targeted green)
- Final workspace gate before PR merge via CI

## Board

0084 → **Completed**. Series M next: body-cloud-links residual, D-0076-attach-content, D-0079-deterministic-key, D-0073-eml.
