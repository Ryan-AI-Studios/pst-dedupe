# Track Completion Review — 0035-CalendarItems

## Verdict: PASS WITH DEFERRED P3

**Final gate:** Codex `gpt-5.6-luna` high — `review.codex.final-r2.md`  
**HEAD at completion:** `1fe475b` on `feat/0035-calendar-items`

## Scope

| Item | Detail |
|---|---|
| Repo | `C:\dev\Dedupe` |
| Branch | `feat/0035-calendar-items` |
| Commits | `a8f38d2` … `1fe475b` |
| Spec / plan | `conductor/0035-CalendarItems/spec.md`, `plan.md` |

## Implementation summary

1. **Schema v16** — `message_class`, `cal_*`, `ics_*` columns; migration + Item map.
2. **pst-reader** — PidTag MessageClass / StartDate / EndDate / Location; `is_calendar_message_class`.
3. **extract-pst** — Calendar branch: `file_category=calendar`, cal fields, attendees JSON, synthesized review text (2 MiB cap), non-email logical hash when no MID.
4. **extract-calendar** — ICS parse (icalendar + chrono-tz); multi-event **archive parent** + **per-VEVENT isolated natives**; resume incomplete expansion; force upsert; TZID DST/unknown/ambiguous honest; RRULE flag only; job `ics_extract`.
5. **process-runner** — `MatterIcsExtractHandler` (feature `calendar`).
6. **dedupe-desk** — Extract ICS button + Review Calendar chip.

## Review rounds

| Round | Result |
|---|---|
| Internal r1 | **FAIL** — P1 parent terminal early; P2 force dupes; P2 PST category test |
| Fix `2cdad45` | Resume/force/PST mapping |
| Internal r2 | **PASS WITH DEFERRED P3** |
| Codex r1 (luna high) | **FAIL** — 5 P2 (attendees, UID path, ATTENDEE, native cap, ambiguous TZ) |
| Fix `37a3f8d` | All 5 P2 |
| Internal r3 | **PASS WITH DEFERRED P3** |
| Codex final | **FAIL** — P2 PST review text uncapped |
| Fix `1fe475b` | 2 MiB PST calendar text cap |
| Codex final-r2 | **PASS WITH DEFERRED P3** (no open P0–P2) |

## DoD matrix (engineering)

| DoD | Status |
|---|---|
| DoD-1 PST calendar | Met |
| DoD-2 ICS + container + TZID | Met |
| DoD-3 Schema v16 | Met |
| DoD-4 Review text + invalidation | Met (ICS apply path; PST insert first-write) |
| DoD-5 Filter calendar | Met |
| DoD-6 Tests §3.9 | Met (prop-level PST; full ICS suite) |
| DoD-7 Gate | Met (workspace hygiene on final commit) |
| DoD-8 Docs/registry | Met at finalize |

## Deferred (docs/deferred.md)

| ID | Item |
|---|---|
| D-0035-01…11 | Planned residuals (PidLid, RRULE expand, UI, Graph, …) |
| D-0035-12 | Embedded VTIMEZONE not used for offset resolve (Codex P3) |
| D-0035-13 | Force multi-child `update_item` does not clear FTS_* immediately (Codex P3) |

## Gates observed

- `cargo test -p extract-calendar` — pass
- `cargo test -p extract-pst --lib` — pass
- Pre-commit hygiene (fmt + clippy + workspace tests) on `1fe475b` — PASSED
- Codex final-r2 — PASS WITH DEFERRED P3

## Completion decision

Engineering complete. Mark conductor **Completed**. Ship via PR.
