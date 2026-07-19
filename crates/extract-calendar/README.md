# extract-calendar

Pure-Rust **iCalendar (ICS) extraction** for Dedupe Desk (track **0035**).

Parses `.ics` / `text/calendar` natives in matter CAS into structured calendar
items with reviewable plain-text bodies. **No Outlook COM**, no Graph API.

## Method stack

| Role | Crate | Pin |
|---|---|---|
| ICS parse | **icalendar** | **0.17.x** (`parser` + `chrono-tz`) |
| TZID → offset | **chrono-tz** | **0.10** (IANA) |

Method id: `ics_icalendar_v1`.

## Container model (multi-event ICS) — **required**

Google/Exchange calendar exports often yield one **massive** multi-event `.ics`.
If every appointment shared that mega-file as `native_sha256`, **0040** produce
of a single meeting would package the whole export (data-breach class failure).

| Rule | Behavior |
|---|---|
| Multi-VEVENT source | Parent becomes `file_category=**archive**`, native = full file digest |
| Per VEVENT | Child `role=attachment`, path `{parent}!/{uid\|vevent-N}.ics` |
| Child native | **Standalone single-event ICS** CAS blob (one VEVENT only) |
| Single-VEVENT file | Leaf item refined to `file_category=calendar`; original or single-event native OK |
| RRULE | `cal_is_recurring=1`; **do not expand** |
| 0040 contract | Produce of a **child** ships the single-event native only. Full export only via archive parent. |

**Forbidden:** assigning the multi-event mega-file hash as every child’s native.

## Timezone resolution

| Input | Result |
|---|---|
| `Z` / UTC | RFC3339 with `Z` |
| `TZID=America/New_York` etc. | Resolved via **chrono-tz** → RFC3339 **with numeric offset** |
| All-day `DATE` | `cal_all_day=1`; start stored as UTC midnight of that date |
| Unknown TZID / floating | `cal_start_at` **null**, `extra_json.cal_tz_unresolved=1` — **no invented offset** |

DST test: same TZID mid-summer vs mid-winter → different offsets.

## Safety limits

| Limit | Default |
|---|---|
| Max native input | 50 MiB |
| Max VEVENTs | 10_000 |
| Max review text / event | 2 MiB |
| Panic isolation | `catch_unwind` per file |

## Job: `ics_extract`

| Item | Value |
|---|---|
| Kind | `ics_extract` |
| Stage | `ics_extract` |
| Params | `{ "force": false, "batch_size": 50 }` |

- Idempotent skip when `ics_source_native_sha256 == native` and status `ok`/`skipped`
- Error status does **not** set source (retryable)
- On text write: NULL `redacted_text_*` (0032); clear `fts_*` (0029)
- **Never** rewrites the source native CAS (children get new CAS digests)

## Error codes

| Code | Meaning |
|---|---|
| `ics_not_ics` | Missing `BEGIN:VCALENDAR` / not ICS |
| `ics_parse_error` | Corrupt / parser panic isolated |
| `ics_limit_exceeded` | Native size or VEVENT count over max |

## Review text

```text
Subject: …
When: <start> – <end> [ALL-DAY]
Where: …
Organizer: …
Attendees: …
Busy: …
Class: …
---
<description / RRULE note>
```

## PST calendar path

PST appointments are handled in **extract-pst** / **pst-reader** (MessageClass
branch), not this crate. Shared schema: `cal_*` columns on items (v16).

## Out of scope (P0)

- RRULE expansion to all occurrences
- Full VTIMEZONE rewrite / exotic floating policies
- Free/busy calendar UI
- Tasks / contacts
