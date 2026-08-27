# 0098 — Template NID / Folder Contents Collision

> Structure follows `templates/0000-Description/spec.md`. DoD is §7.

- **Track ID:** 0098-TemplateNidFolderCollision
- **Execution repo:** `C:\dev\Dedupe`
- **Governance:** this directory in `C:\dev\Dedupe\conductor\` (track registry: `../conductor.md`)
- **Plan-of-record reference:** `C:\dev\Dedupe-plan.md` → Series N residual
- **Cross-repo contract:** n/a
- **Status:** Completed
- **Depends on:** 0068 (template objects) · 0080 (verify via `folders()`) · 0095 (preserve-path layout)
- **Spec authored:** 2026-08-26
- **Series:** N+ (INC0102784 post-0097 verify count gap)
- **Ledger:** `fcdc105c-6185-4ea2-8af0-6968e7ec5930` (`pst-writer`, `BUGFIX`)

---

## 1. Objective

Make unique-pst **verify message counts match written counts** when preserve-path layout
allocates enough folders to reach MS-PST fixed template nidIndex values. Stop the empty
Contents/Hierarchy/Assoc **template objects** at `0x60D`/`0x60E`/`0x60F` from last-winning
over a real folder’s satellite tables.

**Closes:** `D-0098-template-nid-collision` (INC0102784: **4055 written / 4005 found**, all
50 orphans parented to Recoverable Items/Purges NID `0x602`).

---

## 2. Context (read before starting)

### 2.1 Diagnosis (closed, 2026-08-26)

unique-pst verify / QC `structural_digest_pst` sum `folders().message_nids`. NBT on
`output/inc0102784-post-0097/unique.pst`: **4055** `NormalMessage`, **2442** folders walked,
**4005** listed. All 50 NBT orphans have `nid_parent = 0x602` (Purges). That folder’s
contents TC NID is `(0x602 & !0x1F) | 0x0E = 0x60E`.

MS-PST table templates (0068 round 9) are **fixed** NIDs:

| NID | Object |
|---|---|
| `0x60D` | Hierarchy Table Template (nidIndex `0x30`) |
| `0x60E` | Contents Table Template |
| `0x60F` | Associated Contents Table Template |
| `0x610` | Search Contents Table Template |
| `0x671` | Attachment Table Template (nidIndex `0x33`) |
| `0x692` | Recipient Table Template (nidIndex `0x34`) |

`Layout::alloc_nid` started at nidIndex 11 and incremented with **no skip**. Folder index
`0x30` is NID `0x602`. `write_one_folder` writes real contents/hierarchy/assoc, then
templates are `add_node_data`’d at the same NIDs. NBT encode sorts by NID; reader HashMap
**last-wins** → empty 27-column template. `PidTagContentCount` on folder PC `0x602` still
shows 50; `folders()` sees 0 rows. Small CI fixtures never allocated 38+ folders, so 0095
Purges tests did not hit `0x602`.

### 2.2 Product locks

1. Keep the four table templates (and attach/recipient templates) at the **MS-PST fixed NIDs**, always empty.
2. Do **not** “fix” verify by counting NBT `NormalMessage` and ignoring contents TCs — Outlook and QC walk folder contents.
3. Do **not** commit INC* PSTs. Fixture = enough synthetic folders to pass nidIndex `0x30`.
4. No production `unwrap`/`expect`.
5. CRC `not_export_ready`, recipient QC known_gap, `D-0093-attachment-tc-page` stay out of this track.
6. Series O Tauri/frontend IDs were vault-discussed as 0098–0104; **this engineering gap takes 0098**. Frontend, if started later, uses 0105+.

---

## 3. In scope

1. `Layout::alloc_nid` skips reserved nidIndex `0x30` / `0x33` / `0x34`.
2. `add_node_data` (and fixture `add_node`) refuse duplicate NBT NIDs; `write_nbt` fail-closed on duplicate keys.
3. Writer tests: skip unit test; 40-folder preserve-path round-trip (`folders()` count == written; `0x60E` remains empty template schema).
4. Docs: `docs/pst-writer-fidelity-v1.md`, `docs/deferred.md`.

## 4. Out of scope (do NOT do here)

- CRC / AMap integrity (`not_export_ready`).
- Recipient-table truncation / attachment-table heap cap.
- Changing `folders()` to invent NIDs from `nid_parent` when contents TC is empty.
- Series O frontend / Tauri.
- Operator INC* re-smoke as a CI gate (optional after ship; not DoD).

## 5. Preconditions & dependencies

- **P1:** 0068 template NIDs remain required empty TCs.
- *Verified:* 50 Purges messages exist in NBT with parent `0x602`; contents `0x60E` is the template.

## 6. Risks

| Risk | Mitigation |
|---|---|
| Skipping an index shifts later NIDs vs older outputs | Unique-pst is not byte-stable across versions; document. |
| Some other reserved NID still collides | Duplicate-NID insert fails closed. |
| Tests don’t allocate enough folders | 40 top-level preserve folders (user nids start at 14; 0x30 is the 35th). |

## 7. Definition of Done

Complete only when ALL hold:

- [x] **DoD-1 —** `alloc_nid` never returns a NID whose nidIndex is `0x30`, `0x33`, or `0x34`.
- [x] **DoD-2 —** Preserve-path write with ≥40 distinct folders: `folders()` message count equals messages written; NID `0x60E` remains the empty Contents Table Template (0 rows, 27 columns); no user folder is `0x602`.
- [x] **DoD-3 —** Duplicate `add_node_data` NID returns `WriterError::Layout` (fail closed).
- [x] **DoD-4 — Recorded:** `review.md`; registry **Completed**; ledger tx open until git commit (`BUGFIX` `fcdc105c-6185-4ea2-8af0-6968e7ec5930`).

## 8. Verification commands (reference)

```powershell
cargo test -p pst-writer --lib nid_alloc_tests
cargo test -p pst-writer preserve_paths_many_folders_does_not_clobber_contents_template_nid
cargo test -p pst-writer
cargo fmt --all --check
cargo clippy -p pst-writer --all-targets -- -D warnings
```
