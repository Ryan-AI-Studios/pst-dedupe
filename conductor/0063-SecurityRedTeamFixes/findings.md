# 0063 — Security red team findings

Adversarial review + P0/P1 fixes for Series I + RC surfaces. Synthetic tests only; no client data; no secrets in git.

| ID | Severity | Surface | Finding | Evidence | Disposition |
|---|---|---|---|---|---|
| F-0063-01 | **P0** | Platform OIDC | Tenant issuer discovery had no SSRF guard (private/metadata IPs reachable) | `oidc.rs` discovery; unit tests block `127.0.0.1`, `169.254.169.254`, RFC1918, CGNAT, `[::1]` | **Fixed** — `validate_idp_url_for_ssrf` + discovered endpoint re-check; redirects still `Policy::none` |
| F-0063-02 | **P0** | pst-reader | `Vec::with_capacity(lcb_total)` trusted attacker `u32` (up to ~4 GiB) | `ndb/block.rs` XBLOCK/XXBLOCK | **Fixed** — reject `lcb_total > 64 MiB` (`ResourceLimit`) before alloc; runtime assemble cap |
| F-0063-03 | **P0** | pst-reader | NBT/BBT traverse had no visited set / depth limit → crafted cycle hang | `ndb/btree.rs` | **Fixed** — `enter_btree_page` visited + max depth 32; `BtreeCycle` / `ResourceLimit`; production-path `NbtIndex::build` / `BbtIndex::build` synthetic cycle tests |
| F-0063-04 | **P0** | pst-reader | Subnode / leaf-BID walks could cycle | `read_subnode_data`, `list_subnode_entries`, `collect_leaf_data_bids` | **Fixed** — visited BIDs + depth cap 32 |
| F-0063-05 | **P0** | Platform sandbox | `Path::join` absolute RHS override risk; weak absolute non-exist cases | `sandbox.rs` | **Fixed** — early absolute reject; `reject_untrusted_path_component`; regression tests (foreign abs, mixed `..\`, join override doc) |
| F-0063-06 | **P1** | Platform PMK | PMK held as bare `[u8; 32]` without zeroize-on-drop | `pmk.rs` / `Platform` | **Fixed** — `Pmk` with `Zeroize`/`ZeroizeOnDrop`; env string zeroized after parse |
| F-0063-07 | **P1** | Encryption | Passphrase remains in process env after read; heap buffers were bare `String` | `passphrase_from_env`, unlock/create/change paths | **Partial fix + residual** — production unlock/create/change-passphrase paths use `Zeroizing<String>` heap buffers; env residual **D-0063-01**; Desk UI widgets residual **D-0063-05** |
| F-0063-08 | — | Service | Actor spoof under strict mode | `matter-service` `batch_feed_subset_and_actor_spoof_ignored`; routes ignore body `actor` | **Re-verified** — keep green |
| F-0063-09 | — | Service | Bind non-loopback without `allow_lan` | `validate_bind` / `bind_safety_unit` | **Re-verified** — keep green |
| F-0063-10 | — | Service | Dual exclusive lock | soft same-process | **Residual** — **D-0058-04** unchanged |
| F-0063-11 | — | Encryption | Wrong passphrase fail-closed | `matter-core` `wrong_passphrase_fails_closed` | **Re-verified** |
| F-0063-12 | — | Export | unique-pst refuse source overwrite | `pst-dedup-cli` `unique_pst_source_immutability` / `unique_pst_overwrite_refuse_without_flag` | **Re-verified** |
| F-0063-13 | P2 | Supply chain | Re-run audit/deny | CI tools | **Checked** if tools present; deny.toml not weakened |
| F-0063-14 | P2 | Semantic / FTS | Semantic plaintext residual | known | **Deferred** — **D-0057-07** |
| F-0063-15 | P2 | Service LAN | Cleartext HTTP on LAN | intentional | **Deferred** — **D-0058-02** |
| F-0063-16 | **P3** | Platform OIDC | `openidconnect::ClientSecret` retains IdP secret as bare `String` until client Drop; no zeroize API | `oidc.rs` `build_client` / `finish_authorization` | **Deferred D-0063-04 (P3)** — dependency limitation; CoreClient scoped to tight exchange block; local secret zeroized after exchange; full wipe needs upstream |

## Spec §3.2 checklist surfaces (locked) — disposition + evidence

Every locked checklist surface from `spec.md` §3.2 must have an explicit row. “Re-verified” means existing regression tests stayed green; “Fixed” means this track changed code.

### 0057 Encryption

| Control | Disposition | Evidence |
|---|---|---|
| Sealed DB/CAS/FTS when locked; no durable plaintext CAS outside workspace temp policy | **Re-verified** | `matter-core/tests/encryption.rs`: `create_encrypted_open_cas_roundtrip`, `crash_orphan_plain_db_wiped_without_passphrase`, `reseal_roundtrip_no_nonce_reuse_break`; CAS on-disk ciphertext when encryption on |
| Temp/CAS stage under matter workspace; purged on success/fail | **Re-verified** | `encrypted_temp_under_matter_root`; open paths call `cleanup_workspace_temp` / `cleanup_crypto_temps` |
| Passphrase handling: env residual documented or mitigated | **Partial fix + residual** | Heap: `passphrase_from_env() -> Option<Zeroizing<String>>` + service/CLI unlock wraps; env residual **D-0063-01**; Desk UI residual **D-0063-05** |
| Wrong passphrase fail-closed | **Re-verified** | `wrong_passphrase_fails_closed` |
| Key zeroization (DEK/PMK; passphrase buffers after use) | **Fixed / residual honest** | `Dek`/`Pmk` `ZeroizeOnDrop`; production unlock/create/change paths Zeroizing; IdP `ClientSecret` residual **D-0063-04** |

### 0058 Multi-user service

| Control | Disposition | Evidence |
|---|---|---|
| Session required for mutates; body `actor` ignored under strict mode (spoof test) | **Re-verified** | `matter-service/tests/integration.rs` `batch_feed_subset_and_actor_spoof_ignored`; `matter-core` `strict_actor_rejects_free_form_accepts_user_id` |
| Default bind loopback; non-loopback requires `allow_lan` | **Re-verified** | `bind_safety_unit` / `validate_bind` |
| Exclusive matter lock vs second process | **Re-verified** (same-process soft residual D-0058-04) | `exclusive_lock_blocks_second_write_open_sequential` |
| OCC / item locks on contested mutates | **Re-verified** | `occ_stale_version_fails_and_success_bumps` |

### 0059 Platform SSO

| Control | Disposition | Evidence |
|---|---|---|
| Matter register paths confined to `PLATFORM_STORAGE_ROOT` | **Re-verified** | `matter-platform` `bad_path_rejected`, `open_revalidates_storage_root_and_rejects_db_file` |
| Tenant A cannot open/register Tenant B paths | **Re-verified** | `registry_crud_and_isolation` |
| `Path::join` absolute-override / traversal | **Fixed** | `sandbox.rs` absolute reject + `foreign_absolute_rejected`, `mixed_separators_parent_escape_rejected`, `reject_untrusted_path_component_blocks_traversal` |
| IdP secrets not logged; PMK required when ciphertext present; PMK/passphrase zeroized | **Fixed** + **D-0063-04** residual | `Pmk` ZeroizeOnDrop; `resolve_secret_env_and_ciphertext`; OIDC local secret zeroize; ClientSecret residual documented |
| OIDC state/PKCE parameters validated | **Re-verified** | `oidc_pending_is_single_use`; PKCE S256 on start/finish |
| SSRF on tenant-configured OIDC discovery / JWKS URLs | **Fixed** | `ssrf_blocks_private_and_metadata_literals`, public hostname allow/DNS fail-closed; live loopback → `oidc_ssrf` |

### 0060 / produce

| Control | Disposition | Evidence |
|---|---|---|
| Withheld items not produced when gated | **Re-verified** | `matter-produce/tests/integration.rs` `withhold_skipped_not_in_volume`, `fail_if_withheld_aborts` |
| Redacted path does not emit original body when redactions exist | **Re-verified** | `redaction_uses_redacted_text`, `redacted_email_synthetic_eml_uses_redacted_body` |
| Bates uniqueness under concurrent produce residual honesty | **Re-verified** | `bates_start_5001_and_second_volume` (uniqueness/sequencing); concurrent produce residual as prior track honesty |

### 0061 Cloud

| Control | Disposition | Evidence |
|---|---|---|
| No remote SQLite / NFS matter.db path | **Re-verified** | Design lock: matter.db always local SQLite; cloud is CAS blobs only (`cas_backend.rs` / storage config kinds) |
| Blob put integrity (size/hash) fail-closed | **Re-verified** | `matter-storage` `hashing_mismatch_no_store`, `digest_mismatch_leaves_no_object`, `digest_mismatch_deletes_object` |
| Matter-scoped keys/prefixes | **Re-verified** | `key_layout.rs` `key_with_tenant_and_matter`, `key_rejects_dotdot`, `key_rejects_slash_in_tenant` |

### pst-reader / extract

| Control | Disposition | Evidence |
|---|---|---|
| Cycle / depth limits on NBT/BBT/subnode walks | **Fixed** | `enter_btree_page_*` helpers; **production** `nbt_build_detects_self_cycle_page` / `bbt_build_detects_self_cycle_page`; subnode visited + depth 32 |
| Allocation bounds (no unbounded `with_capacity` of attacker sizes) | **Fixed** | XBLOCK/XXBLOCK `lcb_total > 64 MiB` → `ResourceLimit` before alloc |
| Service/worker remains available after single malicious PST open | **Fixed** (bounded fail) | Typed `ResourceLimit` / `BtreeCycle` errors; no infinite loop by design |

### Series K export (light)

| Control | Disposition | Evidence |
|---|---|---|
| unique-pst/eml refuse source overwrite | **Re-verified** | `unique_pst_source_immutability`, `unique_pst_overwrite_refuse_without_flag`, `unique_pst_volume_sibling_input_protected` |
| Report paths cannot clobber inputs | **Re-verified** | `unique_pst_report_write_failure_fail_closed` / report pack isolation tests |

## Residual ledger candidates (D-0063-*)

| Id | Note |
|---|---|
| **D-0063-01** | Matter passphrase may remain in process **env** after unlock (same class as D-0057-09); clear-after-read unsafe with concurrent workers. Heap copies on production unlock paths now use `Zeroizing`. |
| **D-0063-02** | SSRF allows only public DNS hosts; operators misconfiguring public IdP that resolves to private (DNS rebinding) — mitigated by re-check of discovered token/jwks URLs; full multi-resolve race residual |
| **D-0063-03** | XBLOCK 64 MiB cap may reject legitimately huge single blocks; raise only with streaming design |
| **D-0063-04** | **P3** — `openidconnect::ClientSecret` / internal `String` retains IdP client secret until `CoreClient` Drop; **no zeroize API** in dependency. Mitigated by tight exchange-only client scope + local secret zeroize after exchange. Residual heap residue during exchange only. |
| **D-0063-05** | Desk UI passphrase widgets are plain `String` (egui TextEdit); field cleared after submit; heap residue residual until process exit / allocator reuse. |

## Tests added / extended

- `matter-service`: `ssrf_blocks_private_and_metadata_literals`, public hostname allow/DNS fail-closed, live provider loopback → `oidc_ssrf`; OIDC client scoped to exchange block
- `pst-reader`: XBLOCK/XXBLOCK huge `lcbTotal`; btree cycle/depth helpers; **`NbtIndex::build` / `BbtIndex::build` self-cycle production-path tests**
- `matter-platform`: sandbox absolute/mixed/`reject_untrusted_path_component`; `Pmk` ZeroizeOnDrop trait bound
- `matter-core`: `Dek` ZeroizeOnDrop trait bound; `passphrase_from_env` → `Zeroizing<String>`
- CLI/service: unlock/create/change-passphrase paths wrap passphrases in `ZeroizingString`
