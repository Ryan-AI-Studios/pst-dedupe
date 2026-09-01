# 0124 — Review queue chrome (rail, columns, no colliding text)

> Placeholder minted 2026-08-31 from `C:\dev\deviations.md` vs mock
> `C:\dev\dedupe-frontend`. Expand with `/plan-track 124` before Implement.
> Do **not** steal **0117** (virtualization math) or **0118** (window async).

- **Track ID:** 0124-ReviewQueueChrome
- **Status:** Proposed — placeholder
- **Series:** T (mockup chrome fidelity)
- **Depends on:** **0111 / 0117 Completed** · **0123** shell (Go-to slot) · schema **v41**
>
> Live first-pass queue is a **single pane**. Mock is 244px rail + grid.
> Owner HITL (2026-08-31): **column text collides** (From/Subject into
> Fam/Resp). That was not named in `deviations.md`; it is in scope here.

## 1. Objective

Make the first-pass queue **readable and selectable** like the mock without
lying about codes: a 244px rail, a query toolbar whose title is the active
queue, a bulk bar, and a grid whose cells **ellipsis** instead of painting
on top of the next column. Row height stays **32px** (0111/0117
`visible_range`).

Colliding columns are a correctness miss — counsel can tag the wrong
document if From is an X500 DN sitting on top of Privilege.

## 2. In scope (sketch)

### 2.1 Column text must not collide (owner HITL — first DoD)

Live `ui/styles/app.css` (~509–536):

- Tracks: `32px 72px 110px 140px minmax(160px, 1fr) 56px 48px 72px`
  (extras adds four more).
- `.queue-viewport { overflow-x: hidden }`.
- **No** `overflow: hidden` / `text-overflow: ellipsis` on `.queue-row`
  cells. CSS grid items default `min-width: auto`, so long `from_addr`
  (Exchange X500) overflows into Fam / Resp / PRIV.
- Mock `.doc-table-wrap` scrolls; chrome chose a windowed grid instead.

**Lock:** do **not** wrap (breaks 32px virtualization). Each cell:
`min-width: 0` (or `minmax(0, …)` tracks), `overflow: hidden`,
`text-overflow: ellipsis`, `white-space: nowrap`, `title` = full value.
Extras mode must not collide either. HITL: a long X500 From never paints
on Fam/Resp.

SMTP / display-name preference is the same track (ellipsis still required
if From stays X500).

### 2.2 Rail + toolbar + bulk (from deviations.md §3)

1. **244px left rail:** Review queues (Unreviewed, Needs decision, Privilege
   QC, Redaction QC) with counts; Saved searches as named rows (not chips
   beside Unreviewed); Consistency / facets may be **zero** until jobs
   exist — still show the rows. Custodian bars optional if overview already
   has labels.
2. **Toolbar:** title = active queue name + count; readable “Reading as”
   line; facet buttons; Columns menu vs one Lead/QC checkbox.
3. **Go-to** in the **0123** top-bar slot: Control#, Bates, subject.
4. **Bulk:** select-all-matching, Tag…, staging / Privilege QC / batch as
   honest no-ops or wired to existing apply — do not invent a second
   produce pipeline. Keep 0111 privilege-change preview.
5. **Status:** `Rows {start}–{end} of {total}` after Next (0117 range), page
   size, sort, shortcut hint. Enter opens the review window (code already
   navigates on row click — re-HITL; isolate Save-search so it is not the
   page accessible name).
6. **Family members** with `—` date/from/subject on later pages: fill from
   parent or show “— attachment” (mock). Do not invent Bates.

### 2.3 Honesty locks (do not fold the mock blindly)

- Control# remains `review_order` (or real Bates from **0113**
  `latest_control_number` when assigned) — **never** fake `ACME0001`.
- First-pass Privilege column stays **coding** (PRIV), not
  REDACT/WITHHOLD/CANDIDATE. Withhold stays extras / a distinct column.
- No fake QC glyph counts. Nested OR stays **D-0028-02**.

## 3. Out of scope

Shared shell (**0123**). Produce canvas (**0125**). Process visual
(**0126**). Window overlay draw (**0120**). `visible_range` math rewrite
(**0117**). Schema bump. BCC. Vendoring mock tokens.

## 4. DoD (sketch)

- [ ] Long From/Subject never overlap Fam/Resp/Privilege (ellipsis + title;
      32px rows unchanged). Extras columns also clip.
- [ ] Left rail present; Unreviewed default still maps to `preset_uncoded`.
- [ ] Footer shows a truthful row range for the current page, not only
      `N selected · total in queue`.
- [ ] HITL: synthetic matter with X500-length From + a 500+ row Next page
      — no collision; Enter / row click still opens 0112.
