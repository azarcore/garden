//! Rules for keeping designs: when to autosave, how designs are listed,
//! named, imported and exported. Pure logic; storage lives in the web crate.

use loma_core::model::{Document, ObjectKind};
use loma_core::units::format_length;
use uuid::Uuid;

use crate::strings::EN;

/// Quiet time after the last change before an autosave starts.
pub const AUTOSAVE_DELAY_MS: i64 = 800;
/// Wait after a failed save before trying again.
pub const RETRY_DELAY_MS: i64 = 5_000;

/// Decides when to save, given how many changes the editor has made.
#[derive(Debug, Default)]
pub struct Autosave {
    last_seen: u64,
    saved: u64,
    changed_at: i64,
    in_flight: bool,
    /// No save before this time (set after a failure).
    retry_at: i64,
}

impl Autosave {
    pub fn new(saved_change_count: u64) -> Autosave {
        Autosave {
            last_seen: saved_change_count,
            saved: saved_change_count,
            changed_at: 0,
            in_flight: false,
            retry_at: 0,
        }
    }

    /// Call whenever the editor may have changed.
    pub fn observe(&mut self, change_count: u64, now_ms: i64) {
        if change_count != self.last_seen {
            self.last_seen = change_count;
            self.changed_at = now_ms;
        }
    }

    /// A save should start now: unsaved changes, quiet long enough, not mid-gesture, none running.
    pub fn due(&self, now_ms: i64, busy: bool) -> bool {
        self.is_dirty()
            && !self.in_flight
            && !busy
            && now_ms - self.changed_at >= AUTOSAVE_DELAY_MS
            && now_ms >= self.retry_at
    }

    pub fn started(&mut self) {
        self.in_flight = true;
    }

    /// The save of `saved_change_count` finished at `now_ms`. On failure the
    /// design stays dirty and the next attempt waits `RETRY_DELAY_MS`.
    pub fn finished_at(&mut self, saved_change_count: u64, ok: bool, now_ms: i64) {
        self.in_flight = false;
        if ok {
            self.saved = self.saved.max(saved_change_count);
            self.retry_at = 0;
        } else {
            self.retry_at = now_ms + RETRY_DELAY_MS;
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.last_seen > self.saved
    }
}

/// One row of the "My designs" list.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignSummary {
    pub id: String,
    pub name: String,
    pub modified: i64,
    /// Plot size, e.g. "20.00 m × 14.00 m", if a plot has been drawn.
    pub size: Option<String>,
    pub objects: usize,
    /// Set when the stored design can't be opened.
    pub error: Option<String>,
}

pub fn summarize(doc: &Document) -> DesignSummary {
    let size = doc.objects.iter().find_map(|o| match &o.kind {
        ObjectKind::PlotBoundary { shape } => {
            let b = shape.bounding_box();
            Some(format!(
                "{} × {}",
                format_length(b.width(), doc.units),
                format_length(b.height(), doc.units)
            ))
        }
        _ => None,
    });
    DesignSummary {
        id: doc.id.to_string(),
        name: doc.name.clone(),
        modified: doc.modified,
        size,
        objects: doc.objects.len(),
        error: None,
    }
}

pub fn summary_for_error(id: &str, name: &str, modified: i64, err: &str) -> DesignSummary {
    DesignSummary {
        id: id.to_string(),
        name: name.to_string(),
        modified,
        size: None,
        objects: 0,
        error: Some(err.to_string()),
    }
}

/// "just now", "5 min ago", "yesterday", "5 days ago", then the date.
pub fn relative_time(now_ms: i64, then_ms: i64) -> String {
    let diff = (now_ms - then_ms).max(0);
    let (min, hour, day) = (60_000, 3_600_000, 86_400_000);
    if diff < min {
        EN.time_just_now.to_string()
    } else if diff < hour {
        format!("{} min ago", diff / min)
    } else if diff < day {
        format!("{} h ago", diff / hour)
    } else if diff < 2 * day {
        EN.time_yesterday.to_string()
    } else if diff <= 30 * day {
        format!("{} days ago", diff / day)
    } else {
        let (y, m, d) = civil_from_days(then_ms.div_euclid(day));
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// A name for a new design that doesn't clash with existing ones.
pub fn next_design_name(existing: &[&str]) -> String {
    next_name(EN.new_design_name, existing)
}

/// `base`, or `base 2`, `base 3`, … — the first that isn't taken.
pub fn next_name(base: &str, existing: &[&str]) -> String {
    if !existing.contains(&base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !existing.contains(&name.as_str()))
        .unwrap_or_else(|| base.to_string())
}

/// Make an imported document safe to store: never reuse an existing id.
pub fn prepare_import(mut doc: Document, existing_ids: &[Uuid], now_ms: i64) -> Document {
    if existing_ids.contains(&doc.id) {
        doc.id = Uuid::new_v4();
        doc.name = format!("{}{}", doc.name, EN.imported_suffix);
    }
    doc.modified = now_ms;
    doc
}

/// File name for downloads, e.g. "back-garden.loma.json".
pub fn export_file_name(name: &str, extension: &str) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "garden" } else { slug };
    format!("{slug}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::EN;
    use loma_core::command::Command;
    use loma_core::geometry::{Point, Shape};
    use loma_core::model::{Object, ObjectKind};

    #[test]
    fn autosave_waits_for_quiet_and_idle() {
        let mut a = Autosave::new(0);
        assert!(!a.due(0, false));
        a.observe(1, 1000);
        assert!(!a.due(1500, false)); // not quiet long enough
        assert!(!a.due(1900, true)); // busy (dragging)
        assert!(a.due(1900, false));
        a.started();
        assert!(!a.due(5000, false)); // one in flight
        a.observe(2, 1950); // edited while saving
        a.finished_at(1, true, 1960);
        assert!(a.is_dirty());
        assert!(a.due(1950 + AUTOSAVE_DELAY_MS, false));
        a.started();
        a.finished_at(2, true, 3000);
        assert!(!a.is_dirty());
    }

    #[test]
    fn failed_save_stays_dirty() {
        let mut a = Autosave::new(0);
        a.observe(3, 0);
        a.started();
        a.finished_at(3, false, 0);
        assert!(a.is_dirty());
        assert!(a.due(RETRY_DELAY_MS, false));
    }

    #[test]
    fn summary_reads_plot_size_and_count() {
        let mut d = Document::new("Back garden", 0);
        let l = d.layers[0].id;
        let plot = Object::new(
            "Plot",
            ObjectKind::PlotBoundary {
                shape: Shape::rect(Point::ZERO, 20_000., 14_000.),
            },
            l,
        );
        d.apply(
            Command::Insert {
                object: plot,
                index: 0,
            },
            1,
        )
        .unwrap();
        let s = summarize(&d);
        assert_eq!(s.name, "Back garden");
        assert_eq!(s.size.as_deref(), Some("20.00 m × 14.00 m"));
        assert_eq!(s.objects, 1);
        assert_eq!(s.id, d.id.to_string());
        assert!(s.error.is_none());
        assert_eq!(summarize(&Document::new("Empty", 0)).size, None);
        assert_eq!(
            summary_for_error("x", "Old", 5, "boom").error.as_deref(),
            Some("boom")
        );
    }

    #[test]
    fn relative_times() {
        let h = 3_600_000;
        assert_eq!(relative_time(10_000, 0), EN.time_just_now);
        assert_eq!(relative_time(5 * 60_000, 0), "5 min ago");
        assert_eq!(relative_time(3 * h, 0), "3 h ago");
        assert_eq!(relative_time(30 * h, 0), EN.time_yesterday);
        assert_eq!(relative_time(5 * 24 * h, 0), "5 days ago");
        assert_eq!(
            relative_time(1_763_500_000_000 + 40 * 24 * h, 1_763_500_000_000),
            "2025-11-18"
        );
    }

    #[test]
    fn new_design_names_do_not_clash() {
        assert_eq!(next_design_name(&[]), "My garden");
        assert_eq!(next_design_name(&["My garden"]), "My garden 2");
        assert_eq!(
            next_design_name(&["My garden", "My garden 2", "Front"]),
            "My garden 3"
        );
    }

    #[test]
    fn import_never_overwrites_an_existing_design() {
        let d = Document::new("Back", 0);
        let id = d.id;
        let same = prepare_import(d.clone(), &[id], 99);
        assert_ne!(same.id, id);
        assert_eq!(same.name, "Back (imported)");
        assert_eq!(same.modified, 99);
        let fresh = prepare_import(d, &[], 99);
        assert_eq!(fresh.id, id);
        assert_eq!(fresh.name, "Back");
    }

    #[test]
    fn export_names_are_safe() {
        assert_eq!(
            export_file_name("Back garden / 2026!", "loma.json"),
            "back-garden-2026.loma.json"
        );
        assert_eq!(export_file_name("   ", "svg"), "garden.svg");
    }

    #[test]
    fn failed_save_backs_off_before_retrying() {
        let mut a = Autosave::new(0);
        a.observe(3, 0);
        a.started();
        a.finished_at(3, false, 1000);
        assert!(a.is_dirty());
        assert!(!a.due(1250, false)); // no hammering every tick
        assert!(!a.due(1000 + RETRY_DELAY_MS - 1, false));
        assert!(a.due(1000 + RETRY_DELAY_MS, false));
        // a successful save clears the back-off
        a.started();
        a.finished_at(3, true, 7000);
        a.observe(4, 7100);
        assert!(a.due(7100 + AUTOSAVE_DELAY_MS, false));
    }

    #[test]
    fn template_names_do_not_clash() {
        assert_eq!(next_name("Courtyard", &["Courtyard"]), "Courtyard 2");
        assert_eq!(next_name("Courtyard", &[]), "Courtyard");
        assert_eq!(next_design_name(&[]), EN.new_design_name);
    }
}
