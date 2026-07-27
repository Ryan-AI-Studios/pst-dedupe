# 0075 — Keep-Set Winner Policies (Date + Folder Class)

- **Track ID:** 0075-KeepSetWinnerPolicies
- **Status:** Ready
- **Series:** L
- **Depends on:** 0066

## 1. Objective

Replace path-lexicographic “first_seen” surprises (e.g. `INC0102784-2.pst` beating larger primary because `-2` sorts first) with policies operators expect for eDiscovery: **earliest message time**, **folder-class preference**, and clearer docs for path-order first_seen.

## 2. Context

- INC keep-set: 598 winners from `-2`, 3130 from primary; cross-file ties preferred smaller file by absolute path sort.
- Many dups live in Recoverable Items/Purges/Versions — first_seen can crown dumpster copies.
- Existing: `first_seen`, `keep_largest`, `prefer_path`.

## 3. In scope

1. Policy **`earliest_date`** (submit/delivery FILETIME; missing date sorts last).
2. Policy **`folder_class`** or layered preference after fidelity:
   - Primary mailbox store > Archive > Deleted Items > Recoverable Items/Purges > Versions
   - Configurable ordered keywords.
3. Document **`first_seen` = sorted input path order**, not chronological.
4. CLI: `--prefer-folder-class` / policy enum extensions; decision CSV records policy + key used.
5. Tests: path sort vs date; Purges vs Inbox preference.

## 4. Out of scope

- Full custodian ranking UI (Desk later).
- Near-dup conversation collapse (use matter-thread/neardup jobs).

## 5. DoD

- [ ] New policies available on keep-set / unique-pst / unique-eml
- [ ] Deterministic ties documented (path_key, nid)
- [ ] INC-style regression test (synthetic multi-path)
- [ ] README + review.md
