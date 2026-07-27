# 0076 — Content-Hash Tier Hardening

- **Track ID:** 0076-ContentHashTierHardening
- **Status:** Ready
- **Series:** L
- **Depends on:** 0003 / 0065 semantics

## 1. Objective

Reduce Tier-2 false merges/misses by optional **stronger content identity** when Message-ID is absent, without making default scan as slow as full-body hashing every message.

## 2. Context

- Tier 2: subject + date + sender + **body preview (~4KB)** + attach metadata — intentional; documented as not BODY_TRUNCATED.
- INC: mass-mail dups looked correct; same-subject size variance exists across mailboxes.
- Best practice: multi-stage identity (ID → strong hash → soft) like eDiscovery tools; full hash on demand.

## 3. In scope

1. Optional Tier **2.5**: SHA-256 of full body (or body+attach digests) when:
   - no Message-ID, and
   - group size ≥ N candidates under Tier 2, or
   - `--strong-content-hash` global flag.
2. Gate: body length + attach count/size must match before Tier-2 equality (cheap filter).
3. Report: which tier actually bound the group.
4. Benchmark: default path ≤ current scan ±10% on fixtures; strong path documented cost.

## 4. Out of scope

- Semantic near-dup (0023).
- Changing Tier-1 Message-ID.

## 5. DoD

- [ ] Flags + tests for merge/split correctness
- [ ] No silent change to default fixture golden behavior without version note
- [ ] review.md
