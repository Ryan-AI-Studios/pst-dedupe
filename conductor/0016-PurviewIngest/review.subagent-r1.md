# Track Completion Audit — 0016-PurviewIngest (Round 1)

## Verdict: FAIL → fixed on branch

### Must fix
- **P1** Nested ZIP early inventory-skip aborts child expand on resume (`expand.rs`) — **fixed**: inventored containers re-walk via CAS / re-read
- **P2** Resume test only covers flat multi-entry ZIP; need nested mid-archive test — **fixed**: `resume_nested_zip_mid_archive`

### Easy P3 (fix if cheap)
- Nested_zips double-count — **fixed** (once in `expand_zip_file` when depth > 1)
- is_entry_level misleading for bombs — **fixed** (bombs never entry-level)

### Deferred candidates (after P1/P2)
- GP bit 11 approximation — **documented** in README (still approximate)
- Encoding CP437 heuristic nuance
