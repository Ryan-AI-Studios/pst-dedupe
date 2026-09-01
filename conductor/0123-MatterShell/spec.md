# 0123 — Matter shell (shared TopBar + StatusBar)

> Placeholder minted 2026-08-31 from `C:\dev\deviations.md` vs mock
> `C:\dev\dedupe-frontend`. Expand with `/plan-track 123` before Implement.
> Do **not** steal Bugbot residuals **0119–0122**.

- **Track ID:** 0123-MatterShell
- **Status:** Proposed — placeholder
- **Series:** T (mockup chrome fidelity)
- **Depends on:** **0110–0118 Completed** · schema **v41** (no bump)
>
> Source: operator HITL + mock `top_bar.rs` / `status_bar.rs`. Live
> `crates/dedupe-chrome/ui/src/app.rs` is a dark `Dedupe Desk` header; tabs
> live **inside** Home / Process / Review only. Produce and Admin are
> `← Matter home` with no workspace tabs.

## 1. Objective

Once a matter is open, every workspace route uses the **same** mockup shell:
46px top bar (brand · matter name · Process/Review/Produce/Admin · right slot)
and a 30px status bar. Admin stays **inert** as a tab label, not a blank page.
Matters list stays the chrome-only launcher.

This is counsel orientation: coding the wrong workspace because Produce
dropped the tabs is the same honesty class as a silent unique-export drop.

## 2. In scope (sketch)

From `C:\dev\deviations.md` §1, §5, §6 (verified live 2026-08-31):

1. **Shared `TopBar`** on **Home**, Process, Review, Produce, Admin (and Review
   window or a documented exception). Matter name + processed/meta from
   `matter_overview`. Tabs always present; Admin is a span until a later
   Admin batch. Home is a fifth workspace route under that bar (not a tab
   that replaces Process — keep Process/Review/Produce/Admin as the four
   mock tabs; Home can be brand/matter-name click or a documented fifth
   control — pick at `/plan-track 123`, but the page stays).
2. **Shared `StatusBar`** — Hermes rule-of-the-screen on the right; left slot
   is screen-specific (job % / row range / volume). Move Process’s
   “Processing is deterministic…” sentence out of the page body.
3. **Right slot:** Review Go-to (Control# / Bates / subject) may land in
   **0124** if the shell only reserves the slot. Process job readout /
   Produce VOL status this track or the sibling page track — do not invent
   an avatar until identity exists.
4. **Recents BOM** — `recents.rs` `serde_json::from_str` fails on a UTF-8 BOM
   (`expected value at line 1 column 1`). Strip BOM on read; write without BOM.
5. **Home (locked 2026-09-01):** after Open, matter Home stays a workspace
   route **under the same TopBar + StatusBar**. Overview chips stay. Do
   **not** deep-link Open to Process/Review. Matters list remains the
   launcher (no matter shell). Do not leave the placeholder sentence as
   the only body.

**Tokens (locked 2026-09-01 — blue theme):** Keep **IBM Plex** (do **not**
port Archivo). Steal **layout** (46/30, 0-radius, 2px ink). Action /
selection uses mock **ink-navy `#1b3049`**. Cool paper ground. Red remains
privilege / withhold / blocker / draft overlay only — do **not** vendor
coral `#ec3013`. Privilege first-pass column stays 0111 coding (`PRIV`),
not mock REDACT/WITHHOLD.

## 3. Out of scope

Review rail / columns / collision (**0124**). Produce five-step canvas
(**0125**). Process jobs table / drop copy (**0126**). Bugbot **0119–0122**.
Do not vendor `C:\dev\dedupe-frontend`. No schema bump. No BCC. No daemon.

## 4. DoD (sketch)

- [ ] Home, Process, Review, Produce, Admin share one top bar with the four
      mock tabs; Admin is inert (not a dead-looking stub as the only chrome).
      Home sits under that bar (overview chips).
- [ ] Status bar present on those routes; Process Hermes sentence is the
      flag, not body copy.
- [ ] Recents JSON with a UTF-8 BOM loads; writes are BOM-less.
- [ ] Matters list remains the launcher; after Open, Home + Process +
      Review + Produce + Admin share the matter shell (Home under the bar).
