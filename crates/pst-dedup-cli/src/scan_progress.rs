//! Scan / deep-attach stderr cadence (track **0140**).
//!
//! Folder `tracing` events and per-attempt attach-probe `writeln`s are CLI
//! emit-site throttles. CRC first-N stays in `pst_reader::integrity_telemetry`.

use std::time::Duration;

/// Folder INFO cadence: every N folders (plus first/last of the file).
pub const FOLDER_PROGRESS_EVERY: u64 = 250;
/// Folder INFO cadence: minimum interval since last **emit**.
pub const FOLDER_PROGRESS_INTERVAL: Duration = Duration::from_secs(2);
/// Per-attempt probe progress: attempt 1 and every N when `first_n > 0`.
pub const PROBE_PROGRESS_EVERY: u64 = 500;

/// Whether this folder should emit cadence INFO (`-v`).
///
/// `folder_i_1based` and `folder_count` are **per file**. Caller re-arms the
/// timer when this returns true.
pub fn should_emit_folder_progress(
    folder_i_1based: u64,
    folder_count: u64,
    elapsed_since_last: Duration,
) -> bool {
    if folder_i_1based == 0 {
        return false;
    }
    if folder_i_1based == 1 || folder_i_1based == folder_count {
        return true;
    }
    if folder_i_1based.is_multiple_of(FOLDER_PROGRESS_EVERY) {
        return true;
    }
    elapsed_since_last >= FOLDER_PROGRESS_INTERVAL
}

/// Whether a per-attempt deep-attach progress line should print.
///
/// `first_n == 0` (`--crc-log-limit 0`) silences per-attempt lines. Unique-pst
/// end-of-probe summary is not gated here.
pub fn should_emit_probe_progress_line(attempted: u64, first_n: u64) -> bool {
    if first_n == 0 {
        return false;
    }
    attempted == 1 || attempted.is_multiple_of(PROBE_PROGRESS_EVERY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forty_folders_fast_emits_first_and_last_only() {
        let n = 40u64;
        let mut idxs = Vec::new();
        for i in 1..=n {
            if should_emit_folder_progress(i, n, Duration::ZERO) {
                idxs.push(i);
            }
        }
        assert!(
            idxs.len() < n as usize,
            "cadence must not be 1:1, got {idxs:?}"
        );
        assert_eq!(idxs.first().copied(), Some(1));
        assert_eq!(idxs.last().copied(), Some(n));
        assert_eq!(idxs, vec![1, 40]);
    }

    #[test]
    fn every_250th_emits() {
        assert!(should_emit_folder_progress(250, 300, Duration::ZERO));
        assert!(!should_emit_folder_progress(249, 300, Duration::ZERO));
    }

    #[test]
    fn two_second_floor_then_rearm() {
        assert!(should_emit_folder_progress(2, 40, FOLDER_PROGRESS_INTERVAL));
        assert!(!should_emit_folder_progress(3, 40, Duration::ZERO));
    }

    #[test]
    fn folder_i_zero_never_emits() {
        assert!(!should_emit_folder_progress(
            0,
            10,
            FOLDER_PROGRESS_INTERVAL
        ));
    }

    #[test]
    fn probe_limit_zero_silences() {
        assert!(!should_emit_probe_progress_line(1, 0));
        assert!(!should_emit_probe_progress_line(500, 0));
        assert!(!should_emit_probe_progress_line(1000, 0));
    }

    #[test]
    fn probe_default_emits_1_500_1000() {
        let first_n = 10;
        assert!(should_emit_probe_progress_line(1, first_n));
        assert!(!should_emit_probe_progress_line(2, first_n));
        assert!(should_emit_probe_progress_line(500, first_n));
        assert!(should_emit_probe_progress_line(1000, first_n));
        assert!(!should_emit_probe_progress_line(499, first_n));
    }
}
