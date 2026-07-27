# 0079 Plan

## Phase A — Instrument
- [ ] Phase timers in unique_pst_cmd + keep-set materialize
- [ ] Baseline numbers on fixtures/

## Phase B — Materialize
- [ ] Sticky PST opens per path
- [ ] Deduplicate list_attachments
- [ ] Optional parallel materialize + ordered write

## Phase C — Writer hotspots
- [ ] Profile write_unicode_pst_streaming
- [ ] Buffer / CRC / flush adjustments with fidelity tests

## Phase D — Benchmark + gate
- [ ] Document before/after
- [ ] Full unique_pst test suite green
