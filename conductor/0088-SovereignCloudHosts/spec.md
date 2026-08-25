# 0088 — Sovereign Cloud Host Allowlist

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0088-SovereignCloudHosts
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series M continuation after 0087
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0084 · 0085 · 0087 (all **Completed**)
- **Spec authored:** 2026-08-24
- **Series:** M (Unique export fidelity residuals — continuation)
>
> **Review fold-in (2026-08-24):** dual-AI Ready review (`opencode-review.md` + `agy-review.md`) incorporated below.
> Disposition of each claim is in §2.6 (agree / partial / decline with reason).

---

## 1. Objective

Extend offline body-cloud allowlists with **US GCC High / DoD** SharePoint and OneDrive hostname suffixes (plus the documented GCC High/DoD SafeLinks wrapper host) so document-shaped links in those tenants are ledgered the same way as commercial M365 — without claiming Purview collection parity, inventing attachments, or silently covering 21Vianet / `.microsoft` TLD consolidation.

**Closes:** `D-0085-sovereign-cloud-hosts`.

**May open:** `D-0088-usgovcloud-microsoft-tld` (future `.microsoft` content hosts); optional `D-0088-safelinks-sovereign` only for **additional undocumented** SafeLinks wrappers beyond `*.safelinks.protection.office365.us`.

---

## 2. Context (read before starting)

### 2.1 Why this track exists now

| Deferred / ceiling | Severity | Claim |
|---|---|---|
| **D-0085-sovereign-cloud-hosts** | P3 | Commercial allowlist shipped in 0085; sovereign hosts explicitly incomplete |
| Research (2026-07-29 / 0087 Phase-0; confirmed 2026-08-24) | — | Microsoft Learn GCC High (updated 2026-07-01): `*.sharepoint.us`, `admin.onedrive.us`. DoD (updated 2026-06-30): `*.sharepoint-mil.us`, `*.dps.mil` |
| Operator risk | — | Counsel on GCC High / DoD mailboxes gets silent miss on body-cloud CSV |

### 2.2 Live code snapshot (verified 2026-08-24)

| Surface | State |
|---|---|
| `dedup-engine/src/body_cloud_links.rs` `is_commercial_cloud_host` | Commercial SharePoint / OneDrive / `1drv.ms` only — **this is the host table to extend** |
| `is_safelinks_host` | Commercial `safelinks.protection.outlook.com` / `*.safelinks.protection.outlook.com` only |
| Document-shaped path markers | `:w:` / `:x:` / `:p:` / `:b:` / `:u:`; folder `:f:` excluded — **reuse unchanged** |
| 0084 `classify_attach_pc` | Primarily named-prop / `PidTagAttachMethod` / URL-shape (`looks_like_url`). **Also** substring heuristics `t.contains("sharepoint") \|\| t.contains("1drv.")` in `extract_cloud_url` (`pst-reader/src/messaging/attachment.rs`). Not a suffix table. |
| GCC Moderate | Uses **commercial (worldwide)** endpoints — **no extra hosts** |

### 2.3 Product locks

1. **Detect + ledger only** — no network hydration; no inventing Attachment Table rows from body URLs.
2. **Document-shaped gate remains** — sovereign host match alone is not enough; same path/action-token rules as 0085.
3. **US GCC High + DoD only** for this track. 21Vianet (`*.sharepoint.cn`) and retired German sovereign (`*.sharepoint.de`) are **documented exclusions**. GCC Moderate needs nothing extra.
4. Synthetic fixtures only; no client PSTs in git.

### 2.4 Host classes (Phase 0 confirms exact strings + Learn citations)

| Class | Hosts / suffixes | Detection expectation |
|---|---|---|
| GCC High SharePoint / OneDrive | `*.sharepoint.us` (covers `tenant.sharepoint.us` **and** `tenant-my.sharepoint.us`) | **Primary document-shaped hits** — tests **must** include `-my.` personal sites with action tokens |
| GCC High admin | `admin.onedrive.us` | **Harmless include**; Default-category sync/admin — operators should **not** expect body-cloud rows from this host |
| DoD SharePoint | `*.sharepoint-mil.us` (covers `tenant-my.sharepoint-mil.us`) | Primary; tests **must** include `-my.` |
| DoD OneDrive | `*.dps.mil` | Keep; document which path shapes are expected |
| SafeLinks GCC High / DoD | `safelinks.protection.office365.us` and `*.safelinks.protection.office365.us` | **Ship** — unwrap then re-test target against sovereign/commercial document-shaped allowlist. Learn Safe Links article still only documents commercial `*.safelinks.protection.outlook.com`; office365.us is the GCC High/DoD EOP family. Residual only for *other* undocumented wrappers |
| Future `.microsoft` TLD | `*.usgovcloud.microsoft`, `*.usgovcloud-static.microsoft`, `*.usgovcloud-usercontent.microsoft` (Learn GCC High ID 23) | **Out of P0** — open `D-0088-usgovcloud-microsoft-tld`. Do not guess path shapes |

**SafeLinks value bound:** current Microsoft behavior is that Safe Links **no longer wraps URLs pointing to SharePoint or OneDrive sites** (Learn, Safe Links about, updated 2026-05-22). Unwrap path is mainly **historical mail**. Document this in the residual / runbook so the gap is not overstated.

**Implementation hint (not a crate refactor):** prefer a `const` suffix array + single matcher in `body_cloud_links.rs` so future host additions are one-line.

### 2.5 Reader substring vs engine suffix (locked)

- **Body-cloud P0** is the suffix table in `body_cloud_links.rs`.
- **Do not** add a `dedup-engine` dependency into `pst-reader`.
- **If cheap in this track:** tighten `extract_cloud_url` substring `contains("sharepoint")` / `contains("1drv.")` to **suffix-equivalent** checks that include the new sovereign suffixes (false-positive: `notsharepoint.attacker.com`). If that expands beyond a small local helper in `pst-reader`, **open residual** rather than grow 0088 into a shared-classifier crate.

### 2.6 Dual-AI review disposition (2026-08-24)

| # | Claim | Source | Disposition | Spec landing |
|---|---|---|---|---|
| O1 | Record `.microsoft` TLD consolidation as future residual | opencode | **Agree** | §2.4; `D-0088-usgovcloud-microsoft-tld` |
| O2 | `admin.onedrive.us` / `*.dps.mil` low vs `-my.sharepoint*` primary | opencode | **Agree** | §2.4 expectation column |
| O3 | Dual host table is a non-issue; 0084 has no suffix table | opencode | **Partial** | Named-prop path has no suffix table; `extract_cloud_url` **does** substring-match — §2.2 / §2.5 |
| O4 | GCC Moderate uses commercial endpoints | opencode | **Agree** | §2.2; §2.3 lock 3 |
| O5 | SafeLinks no longer wraps SP/OD; residual is historical | opencode | **Agree** | §2.4 bound |
| O6 | Suffix array + matcher; negative `:f:` / bare-root per suffix | opencode | **Agree** | §2.4 hint; DoD-2 |
| A1 | Ship `safelinks.protection.office365.us` now; do not defer | agy | **Agree (ship host)** | §2.4 SafeLinks row; residual only for *other* wrappers |
| A2 | Test `-my.sharepoint.us` / `-my.sharepoint-mil.us` | agy | **Agree** | DoD-1 |
| A3 | Unify reader + engine into one shared `is_cloud_host` | agy | **Partial / decline crate merge** | §2.5 — cheap local tighten OK; no new shared crate |
| A4 | Document 21Vianet / retired DE exclusions | agy | **Agree** | §2.3; runbook |
| A5 | Add `*.office365.us` as a SharePoint host class | agy | **Decline** | EOP/SafeLinks family, not document SharePoint sites; SafeLinks host is explicit |

---

## 3. In scope

1. Extend `is_commercial_cloud_host` (rename if needed) and `is_safelinks_host` for §2.4 hosts.
2. Unit tests: sovereign document-shaped URLs hit (including `-my.` and `:x:`); SafeLinks unwrap of office365.us → `*.sharepoint.us` document URL; bare roots / `:f:` per new suffix excluded; commercial regressions green.
3. Optional cheap suffix tighten in `extract_cloud_url` (§2.5).
4. Operator/docs: coverage, GCC Moderate note, 21Vianet exclusion, SafeLinks historical bound, `.microsoft` residual.
5. Close `D-0085-sovereign-cloud-hosts`; open `D-0088-usgovcloud-microsoft-tld`.

## 4. Out of scope

- Network hydrate / Mode A promote from body-only hits (0085 policy).
- Full NPMAP write (`0092` / `D-0084-cloud-named-prop-write`).
- GUI surfaces for body-cloud CSV.
- Claiming Purview sovereign eDiscovery parity.
- 21Vianet / EU Data Boundary / `.microsoft` content hosts.
- New shared crate / `pst-reader` depending on `dedup-engine`.

## 5. Preconditions & dependencies

- **P1:** 0085 Completed (commercial body-cloud path).
- *Verified:* `is_commercial_cloud_host` / `is_safelinks_host` live in `body_cloud_links.rs`.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Over-broad `*.us` / `*.mil` | Exact documented suffixes only; document-shaped gate |
| Stale Microsoft endpoint docs | Phase 0 cite Learn pages + access date |
| Reader substring false positives | Cheap local suffix tighten or residual — not a shared crate |

## 7. Definition of Done

- [x] **DoD-1 — Hosts:** GCC High + DoD SharePoint document-shaped URLs (including `<tenant>-my.sharepoint.us` and `<tenant>-my.sharepoint-mil.us`) are detected by `scan_body_cloud_links`. Unwrapped `*.safelinks.protection.office365.us` → document-shaped `*.sharepoint.us` hits.
- [x] **DoD-2 — Proportionality:** Bare sovereign site roots / `:f:` folder shares still excluded **for each new suffix**.
- [x] **DoD-3 — Regression:** Existing commercial 0085 unit tests remain green.
- [x] **DoD-4 — Honesty:** Close `D-0085-sovereign-cloud-hosts`. Open `D-0088-usgovcloud-microsoft-tld`. Document 21Vianet exclusion, GCC Moderate, SafeLinks historical bound, `admin.onedrive.us` expectation.
- [x] **DoD-5 — Recorded:** `review.md`; conductor **Completed**; ledger TX committed (`FEATURE` or `DOCS` as appropriate for the code change session).

## 8. Verification commands

```powershell
cargo test -p dedup-engine -- body_cloud
cargo test -p pst-reader -- attachment
cargo fmt --all --check
cargo clippy -p dedup-engine -p pst-reader --all-targets -- -D warnings
ledgerful verify
```
