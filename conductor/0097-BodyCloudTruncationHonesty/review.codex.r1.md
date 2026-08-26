# Track Completion Audit — 0097-BodyCloudTruncationHonesty

## Verdict: FAIL

Two P2 correctness gaps remain in combined-cap and window-edge paths.

## Findings

### [P2] Tail and post-cap probes discard over-length marker metadata

Confidence: High  
Requirement: §2.5 URL-over-length semantics; §2.6 tail rescan; DoD-1 through DoD-3  
Location: `crates/dedup-engine/src/body_cloud_links.rs:169-174,330-367`; `crates/pst-dedup-cli/src/unique_pst_cmd.rs:2528-2539`

Problem: `has_document_candidates` returns only `bool`. When it finds an over-length URL in the body tail, callers set only `window_dropped`; they do not set `url_truncated` or retain the 2048-character prefix. The same occurs for an over-length candidate beyond the 50-hit cutoff.

Failure scenario: A >100k body with a 2,500-character SharePoint URL in its tail emits only `BODY_CLOUD_LINK_WINDOW` with an empty URL, instead of including `BODY_CLOUD_LINK_URL_TRUNCATED` and the required prefix.

Correction: Return candidate metadata from the shared probe, including over-length status and the first prefix. Preserve the single-marker and no-kept-hit rules.

Verification: Add tail-over-length and 50-kept-plus-over-length scanner/CLI tests.

Deferrable: No

### [P2] Window-edge guard marks deduplicated URLs as dropped

Confidence: High  
Requirement: `truncated` must represent actually dropped document-shaped candidates; exact deduplication must remain honored  
Location: `crates/dedup-engine/src/body_cloud_links.rs:236-253,284-289,312-319`

Problem: `handle_window_edge_bare` does not consult `acc.seen`. A URL already kept earlier in the message can be repeated across the 100k boundary; the repeated occurrence then sets `window_dropped=true` and produces a false marker.

Failure scenario: A message contains a kept URL in the prefix and the same URL again, cut by the window boundary. The scan reports truncation and increments the truncated-message counter despite no new unique candidate being dropped.

Correction: Pass the seen set or normalized final URL into the edge handler and suppress the drop flag for already-seen URLs while still rejecting the cut prefix as a real hit.

Verification: Add a duplicate boundary-crossing test asserting no marker and `truncated=false`.

Deferrable: No
