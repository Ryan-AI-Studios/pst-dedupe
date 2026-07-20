# matter-entity

Offline **entity / PII packs** for Dedupe Desk (track **0046**).

## What it does

- Scans item **plain text** (CAS via `text_sha256`) and **subject** with built-in regex packs
- Validates cards with **Luhn** (+ length/IIN) and SSNs with light invalid rules
- Stores **masked_value + match_hash** only — **never** cleartext PAN/SSN in SQLite
- Job kind: `entity_scan` (opt-in; not silent on extract)

## Built-in packs

| pack_id | entity_type | Notes |
|---|---|---|
| `email` | `email` | Strip edge punctuation; case-fold hash; **domain fully visible** in mask |
| `phone_us` | `phone_us` | 10 digits after strip; optional leading `1` |
| `ssn_us` | `ssn_us` | Reject area 000/666/9xx, group 00, serial 0000 |
| `credit_card` | `credit_card` | Luhn + length 13–19 + rough IIN |
| `currency_usd` | `currency_usd` | Parseable `$` / `USD` amounts |

Each pack embeds `PACK_VERSION` in hit rows and audit events.

## Architectural locks

1. **Offline regex only** — no NER/ML/cloud
2. **Rust `regex` crate only** (finite automata / linear-time) — **no ReDoS** from backtracking engines
3. **Mask + hash** storage only
4. **Email domain unmasked** in display mask (`b***@competitor.com`)
5. **Fingerprint-aware idempotency** via `items.entity_scanned_text_sha256` (packs + fields + trunc)
6. Offsets are **UI hints** — OOB must not panic
7. Opt-in job on blocking worker

## Job params

```json
{
  "packs": ["email", "phone_us", "ssn_us", "credit_card", "currency_usd"],
  "max_text_bytes": 2000000,
  "reset": false,
  "batch_size": 100,
  "scope": "all"
}
```

### Idempotency (`reset: false`)

Skip only when stored `entity_scanned_text_sha256` equals the **full-success** scan fingerprint for the **current** candidate + params:

```
escan_v1|packs=<id@ver sorted>|body=<hex|->|trunc=full|subj=<hex|->|from=<hex|->
```

Fingerprint components:

| Part | Meaning |
|---|---|
| `packs=` | Sorted `pack_id@pack_version` for the enabled set |
| `body=` | Body CAS digest, `-` if no body, or `err:<digest>` after CAS load failure |
| `trunc=` | `full` on complete body read; `{max_text_bytes}` when truncated |
| `subj=` / `from=` | SHA-256 of trimmed field content, or `-` if empty |

Rules:

1. NULL marker → scan  
2. Stored fingerprint equals full-success form (`trunc=full`, clean `body=digest` or `body=-`) → **skip**  
3. Pack set/version change, subject/from content change, body digest change → rescan  
4. Prior `trunc=N` never matches full-success → **retry** (larger `max_text_bytes` can find PII past the old cap)  
5. Prior `body=err:…` never matches clean body → **retry** CAS (no permanent skip on load failure)  
6. After process: store fingerprint reflecting actual outcome (including `err:` / `trunc=N`)

Legacy bare digests / `subject:` markers do not match `escan_v1` → one free rescan.

### `reset: true`

Clears all matter entity hits + item entity columns, then scans all candidates.

## CLI recipe (0045 generic job)

```powershell
.\target\release\pst-dedup.exe job run --path $m --kind entity_scan --json
# or with params:
.\target\release\pst-dedup.exe job run --path $m --kind entity_scan --params-json '{"reset":false}' --json
```

## Honesty

- Regex + Luhn ≠ forensic-grade PII confirmation  
- False positives expected (document numbers, etc.)  
- Not PHI / national-ID specialized  
- HTML body scan and NER are deferred  

## Fixtures

Synthetic only: `fixtures/entity/sample_pii.txt` (fake SSNs/cards designed for tests).
