//! ICS parse + single-event native isolation + TZID resolution.

use std::panic::{catch_unwind, AssertUnwindSafe};

use chrono::{NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use icalendar::{
    Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime, Event, EventLike,
};
use serde_json::json;

use crate::error::{Error, Result};
use crate::limits::{methods, MAX_VEVENTS};

/// Mapped fields for one VEVENT / calendar child.
#[derive(Debug, Clone, Default)]
pub struct CalendarEventFields {
    pub subject: Option<String>,
    pub description: Option<String>,
    pub message_class: Option<String>,
    pub cal_start_at: Option<String>,
    pub cal_end_at: Option<String>,
    pub cal_all_day: Option<i64>,
    pub cal_location: Option<String>,
    pub cal_organizer: Option<String>,
    pub cal_attendees_json: Option<String>,
    pub cal_busy_status: Option<String>,
    pub cal_is_recurring: Option<i64>,
    pub cal_recurrence_id: Option<String>,
    pub cal_uid: Option<String>,
    pub cal_extract_method: Option<String>,
    pub attendee_addrs: Vec<String>,
    pub rrule_text: Option<String>,
    /// Set when TZID could not be resolved (honest null times).
    pub tz_unresolved: bool,
    pub unresolved_tzid: Option<String>,
}

/// One parsed VEVENT with isolated single-event ICS bytes.
#[derive(Debug, Clone)]
pub struct ParsedVEvent {
    pub index: usize,
    pub fields: CalendarEventFields,
    /// Standalone single-event ICS bytes for CAS native.
    pub single_event_ics: Vec<u8>,
}

/// Full ICS parse result.
#[derive(Debug, Clone)]
pub struct ParsedIcs {
    pub events: Vec<ParsedVEvent>,
    pub method: String,
    /// True when more than one VEVENT (container model).
    pub is_container: bool,
    /// Original calendar PRODID/VERSION for rebuild.
    pub prodid: Option<String>,
    pub version: Option<String>,
}

/// Result of mapping times.
#[derive(Debug, Clone)]
struct ResolvedTime {
    rfc3339: Option<String>,
    all_day: bool,
    tz_unresolved: bool,
    unresolved_tzid: Option<String>,
}

/// Parse ICS bytes into structured events (panic-isolated wrapper).
pub fn extract_ics_catch_unwind(bytes: &[u8]) -> Result<ParsedIcs> {
    match catch_unwind(AssertUnwindSafe(|| parse_ics(bytes))) {
        Ok(r) => r,
        Err(_) => Err(Error::parse("panic during ICS parse")),
    }
}

/// Parse ICS with default limits.
pub fn parse_ics(bytes: &[u8]) -> Result<ParsedIcs> {
    parse_ics_with_limits(bytes, MAX_VEVENTS)
}

/// Parse ICS with injectable VEVENT cap.
pub fn parse_ics_with_limits(bytes: &[u8], max_vevents: usize) -> Result<ParsedIcs> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| Error::parse(format!("ICS is not valid UTF-8: {e}")))?;
    let cal: Calendar = text
        .parse()
        .map_err(|e| Error::parse(format!("icalendar parse failed: {e}")))?;

    let prodid = cal.property_value("PRODID").map(|s| s.to_string());
    let version = cal
        .property_value("VERSION")
        .map(|s| s.to_string())
        .or_else(|| Some("2.0".into()));

    // Collect raw VTIMEZONE blocks from source for single-event rebuild
    // (best-effort: include matching TZID lines from original text).
    let vtimezone_blobs = extract_vtimezone_blobs(text);

    let mut events = Vec::new();
    let mut index = 0usize;
    for component in cal.components.iter() {
        if let CalendarComponent::Event(ev) = component {
            if events.len() >= max_vevents {
                return Err(Error::limit(format!(
                    "VEVENT count exceeds max {max_vevents}"
                )));
            }
            let fields = map_event(ev);
            let single = build_single_event_ics(
                ev,
                version.as_deref().unwrap_or("2.0"),
                prodid
                    .as_deref()
                    .unwrap_or("-//Dedupe//extract-calendar//EN"),
                &vtimezone_blobs,
            );
            events.push(ParsedVEvent {
                index,
                fields,
                single_event_ics: single,
            });
            index += 1;
        }
    }

    if events.is_empty() {
        return Err(Error::parse("no VEVENT components found"));
    }

    Ok(ParsedIcs {
        is_container: events.len() > 1,
        events,
        method: methods::ICS_ICALENDAR_V1.into(),
        prodid,
        version,
    })
}

fn map_event(ev: &Event) -> CalendarEventFields {
    let mut fields = CalendarEventFields {
        subject: ev.get_summary().map(|s| s.to_string()),
        description: ev.get_description().map(|s| s.to_string()),
        message_class: Some("VEVENT".into()),
        cal_location: ev.get_location().map(|s| s.to_string()),
        cal_uid: ev.get_uid().map(|s| s.to_string()),
        cal_extract_method: Some(methods::ICS_ICALENDAR_V1.into()),
        ..Default::default()
    };

    // Organizer
    if let Some(org) = ev.property_value("ORGANIZER") {
        fields.cal_organizer = Some(strip_mailto(org));
    }

    // Attendees
    let attendees = ev.get_attendees();
    let mut att_json = Vec::new();
    let mut addrs = Vec::new();
    for a in attendees {
        // Attendee Display: try common accessors via property string form.
        let raw = format!("{a:?}");
        let _ = raw; // Debug only fallback
    }
    // Prefer multi_properties ATTENDEE values
    if let Some(props) = ev.multi_properties().get("ATTENDEE") {
        for p in props {
            let val = p.value();
            let addr = strip_mailto(val);
            addrs.push(addr.clone());
            let mut obj = json!({ "addr": addr });
            if let Some(cn) = p.params().get("CN") {
                obj["name"] = json!(cn.value());
            }
            if let Some(role) = p.params().get("ROLE") {
                obj["role"] = json!(role.value());
            }
            if let Some(ps) = p.params().get("PARTSTAT") {
                obj["partstat"] = json!(ps.value());
            }
            att_json.push(obj);
        }
    } else if let Some(val) = ev.property_value("ATTENDEE") {
        let addr = strip_mailto(val);
        addrs.push(addr.clone());
        att_json.push(json!({ "addr": addr }));
    }
    if !att_json.is_empty() {
        fields.cal_attendees_json = Some(serde_json::to_string(&att_json).unwrap_or_default());
    }
    fields.attendee_addrs = addrs;

    // RRULE — flag only, do not expand
    if let Some(rrule) = ev.property_value("RRULE") {
        fields.cal_is_recurring = Some(1);
        fields.rrule_text = Some(rrule.to_string());
    } else {
        fields.cal_is_recurring = Some(0);
    }

    // RECURRENCE-ID
    if let Some(rid) = ev.get_recurrence_id() {
        fields.cal_recurrence_id = Some(format!("{rid:?}"));
    } else if let Some(uid) = fields.cal_uid.clone() {
        fields.cal_recurrence_id = Some(uid);
    }

    // Busy / TRANSP
    if let Some(t) = ev.property_value("TRANSP") {
        let upper = t.to_ascii_uppercase();
        let busy = match upper.as_str() {
            "TRANSPARENT" => "free".to_string(),
            "OPAQUE" => "busy".to_string(),
            _ => t.to_string(),
        };
        fields.cal_busy_status = Some(busy);
    }

    let start = resolve_date_perhaps(ev.get_start());
    let end = resolve_date_perhaps(ev.get_end());
    fields.cal_start_at = start.rfc3339;
    fields.cal_end_at = end.rfc3339;
    if start.all_day || end.all_day {
        fields.cal_all_day = Some(1);
    } else if fields.cal_start_at.is_some() {
        fields.cal_all_day = Some(0);
    }
    fields.tz_unresolved = start.tz_unresolved || end.tz_unresolved;
    fields.unresolved_tzid = start.unresolved_tzid.or(end.unresolved_tzid);

    fields
}

fn resolve_date_perhaps(dpt: Option<DatePerhapsTime>) -> ResolvedTime {
    let Some(dpt) = dpt else {
        return ResolvedTime {
            rfc3339: None,
            all_day: false,
            tz_unresolved: false,
            unresolved_tzid: None,
        };
    };
    match dpt {
        DatePerhapsTime::Date(d) => ResolvedTime {
            // All-day: UTC midnight of the date (documented convention).
            rfc3339: Some(format!(
                "{:04}-{:02}-{:02}T00:00:00Z",
                d.year(),
                d.month(),
                d.day()
            )),
            all_day: true,
            tz_unresolved: false,
            unresolved_tzid: None,
        },
        DatePerhapsTime::DateTime(cdt) => resolve_calendar_datetime(cdt),
    }
}

fn resolve_calendar_datetime(cdt: CalendarDateTime) -> ResolvedTime {
    match cdt {
        CalendarDateTime::Utc(dt) => ResolvedTime {
            rfc3339: Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            all_day: false,
            tz_unresolved: false,
            unresolved_tzid: None,
        },
        CalendarDateTime::Floating(ndt) => {
            // Floating local time — do not invent offset; store null + flag via caller.
            let _ = ndt;
            ResolvedTime {
                rfc3339: None,
                all_day: false,
                tz_unresolved: true,
                unresolved_tzid: Some("floating".into()),
            }
        }
        CalendarDateTime::WithTimezone { date_time, tzid } => resolve_tzid_local(date_time, &tzid),
    }
}

fn resolve_tzid_local(ndt: NaiveDateTime, tzid: &str) -> ResolvedTime {
    // Strip common Outlook prefixes
    let cleaned = tzid.trim().trim_matches('"');
    let cleaned = cleaned
        .strip_prefix("tzid:")
        .or_else(|| cleaned.strip_prefix("TZID:"))
        .unwrap_or(cleaned)
        .trim();

    match cleaned.parse::<Tz>() {
        Ok(tz) => match tz.from_local_datetime(&ndt) {
            chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => {
                ResolvedTime {
                    rfc3339: Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                    all_day: false,
                    tz_unresolved: false,
                    unresolved_tzid: None,
                }
            }
            chrono::LocalResult::None => ResolvedTime {
                rfc3339: None,
                all_day: false,
                tz_unresolved: true,
                unresolved_tzid: Some(cleaned.into()),
            },
        },
        Err(_) => ResolvedTime {
            rfc3339: None,
            all_day: false,
            tz_unresolved: true,
            unresolved_tzid: Some(cleaned.into()),
        },
    }
}

fn strip_mailto(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("mailto:") {
        rest.to_string()
    } else if let Some(rest) = t.strip_prefix("MAILTO:") {
        rest.to_string()
    } else {
        t.to_string()
    }
}

/// Extract VTIMEZONE component text blobs from raw ICS for inclusion in
/// single-event rebuilds (best-effort string scan).
fn extract_vtimezone_blobs(ics: &str) -> Vec<String> {
    let mut out = Vec::new();
    let upper = ics.to_ascii_uppercase();
    let mut search_from = 0usize;
    while let Some(rel) = upper[search_from..].find("BEGIN:VTIMEZONE") {
        let start = search_from + rel;
        let end_rel = upper[start..].find("END:VTIMEZONE");
        let Some(end_rel) = end_rel else { break };
        let end = start + end_rel + "END:VTIMEZONE".len();
        // Include trailing CRLF if present
        let end = if ics[end..].starts_with("\r\n") {
            end + 2
        } else if ics[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        out.push(ics[start..end].to_string());
        search_from = end;
    }
    out
}

/// Build a standalone single-event ICS byte blob (produce-safe).
pub fn build_single_event_ics(
    event: &Event,
    version: &str,
    prodid: &str,
    vtimezones: &[String],
) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\n");
    out.push_str(&format!("VERSION:{version}\r\n"));
    out.push_str(&format!("PRODID:{prodid}\r\n"));
    out.push_str("CALSCALE:GREGORIAN\r\n");
    for vt in vtimezones {
        // Ensure CRLF line endings-ish; keep as-is if already good.
        if !vt.ends_with('\n') {
            out.push_str(vt);
            out.push_str("\r\n");
        } else {
            out.push_str(vt);
        }
    }
    // Serialize the single event via icalendar Display
    let ev_str = event.to_string();
    // event.to_string() typically includes BEGIN:VEVENT … END:VEVENT
    out.push_str(&ev_str);
    if !out.ends_with("\n") {
        out.push_str("\r\n");
    }
    out.push_str("END:VCALENDAR\r\n");
    out.into_bytes()
}

/// Count VEVENT occurrences via re-parse of single-event bytes (produce safety).
pub fn count_vevents_in_ics(bytes: &[u8]) -> Result<usize> {
    let parsed = parse_ics(bytes)?;
    Ok(parsed.events.len())
}

// Helper for NaiveDate formatting without chrono Datelike import noise
trait Ymd {
    fn year(&self) -> i32;
    fn month(&self) -> u32;
    fn day(&self) -> u32;
}
impl Ymd for NaiveDate {
    fn year(&self) -> i32 {
        use chrono::Datelike;
        Datelike::year(self)
    }
    fn month(&self) -> u32 {
        use chrono::Datelike;
        Datelike::month(self)
    }
    fn day(&self) -> u32 {
        use chrono::Datelike;
        Datelike::day(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_event() {
        let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\n\
BEGIN:VEVENT\r\nUID:single-1\r\nSUMMARY:Hello\r\n\
DTSTART:20260718T150000Z\r\nDTEND:20260718T160000Z\r\n\
DESCRIPTION:BODY_MARKER\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let p = parse_ics(ics).expect("parse");
        assert!(!p.is_container);
        assert_eq!(p.events.len(), 1);
        assert_eq!(p.events[0].fields.subject.as_deref(), Some("Hello"));
        assert_eq!(
            p.events[0].fields.cal_start_at.as_deref(),
            Some("2026-07-18T15:00:00Z")
        );
        let n = count_vevents_in_ics(&p.events[0].single_event_ics).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn tzid_dst_summer_vs_winter() {
        let summer = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nUID:s\r\nSUMMARY:Summer\r\n\
DTSTART;TZID=America/New_York:20260715T100000\r\n\
DTEND;TZID=America/New_York:20260715T110000\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";
        let winter = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Winter\r\n\
DTSTART;TZID=America/New_York:20260115T100000\r\n\
DTEND;TZID=America/New_York:20260115T110000\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";
        let s = parse_ics(summer).unwrap();
        let w = parse_ics(winter).unwrap();
        let so = s.events[0].fields.cal_start_at.as_deref().unwrap();
        let wo = w.events[0].fields.cal_start_at.as_deref().unwrap();
        // Summer EDT -04:00, winter EST -05:00
        assert!(
            so.contains("-04:00") || so.ends_with("-04:00"),
            "summer={so}"
        );
        assert!(
            wo.contains("-05:00") || wo.ends_with("-05:00"),
            "winter={wo}"
        );
        assert_ne!(so, wo);
    }

    #[test]
    fn unknown_tzid_no_invented_offset() {
        let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nUID:u\r\nSUMMARY:X\r\n\
DTSTART;TZID=Not/ARealZone:20260718T100000\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";
        let p = parse_ics(ics).unwrap();
        assert!(p.events[0].fields.cal_start_at.is_none());
        assert!(p.events[0].fields.tz_unresolved);
    }

    #[test]
    fn rrule_not_expanded() {
        let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nUID:r\r\nSUMMARY:Weekly\r\n\
DTSTART:20260101T100000Z\r\n\
RRULE:FREQ=WEEKLY;COUNT=100\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";
        let p = parse_ics(ics).unwrap();
        assert_eq!(p.events.len(), 1);
        assert_eq!(p.events[0].fields.cal_is_recurring, Some(1));
        assert!(p.events[0]
            .fields
            .rrule_text
            .as_deref()
            .unwrap()
            .contains("FREQ=WEEKLY"));
    }
}
