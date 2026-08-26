# Track Completion Audit — 0097-BodyCloudTruncationHonesty

## Verdict: FAIL

The two r1 P2s are fixed. One new P2 remains in the combined max-links/window-edge path.

## Findings

### [P2] 50-hit early return bypasses the window-edge duplicate guard

Confidence: High  
Requirement: §§2.5–2.7; DoD-1 and DoD-3  
Location: `crates/dedup-engine/src/body_cloud_links.rs:297-307,323-334,347-379`

Problem: After the 50th kept hit, both collectors call `note_unseen_in(text, false)` and return before `handle_window_edge_bare` runs. The probe scans the truncated 100k window and can classify a cut `.xls` prefix as a new candidate even when the full URL continuing past the boundary is already present in `seen`.

Failure scenario: A body contains 50 kept URLs, then repeats one URL across the window boundary as `book.xls` + `x?d=1`. The scanner emits `truncated=true` and a `BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED` marker despite no unique document candidate being dropped.

Correction: Preserve window-edge context in the post-50 probe, or exclude terminal cut bare matches from that probe and let the edge handler classify them against `seen`. Add scanner and CLI tests with 50 hits plus a duplicate cut URL.

Verification: Re-run scanner tests and `cargo test -p pst-dedup-cli --test unique_pst`.

Deferrable: No

### Prior r1 findings — fixed

- Tail/post-cap probes now retain over-length metadata and the 2048-character prefix.
- Window-edge handling now checks `acc.seen`.
- Current scanner tests covering both fixes passed.
