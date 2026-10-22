use super::record::Record;

pub(crate) const LIMIT: usize = 4096;
const STORE_CAP: usize = 4096;
pub(crate) const MAX_STAGES: usize = 16;

pub(crate) fn cap(text: String) -> String {
    if text.len() <= STORE_CAP {
        return text;
    }
    let mut text = text;
    let keep = text.floor_char_boundary(STORE_CAP);
    text.truncate(keep);
    text
}

pub(crate) fn encode(record: &Record) -> Vec<u8> {
    encode_within(record, LIMIT)
}

fn encode_within(record: &Record, limit: usize) -> Vec<u8> {
    let bytes = serde_json::to_vec(record).unwrap_or_default();
    if bytes.len() <= limit {
        return bytes;
    }
    let mut copy = record.clone();
    fit(&mut copy, limit);
    let bytes = serde_json::to_vec(&copy).unwrap_or_default();
    if bytes.len() <= limit {
        bytes
    } else {
        Vec::new()
    }
}

#[derive(Clone, Copy)]
enum Which {
    Label,
    Detail,
    StageNote,
    Error,
    Device,
    Out,
    App,
    Invoker,
    Parent,
    Tool,
    Host,
    Id,
    Stage,
    Unit,
    AsName,
}

const PREFERRED: [Which; 4] = [Which::Label, Which::Detail, Which::StageNote, Which::Error];

const EXTRA: [Which; 11] = [
    Which::Device,
    Which::Out,
    Which::App,
    Which::Invoker,
    Which::Parent,
    Which::Tool,
    Which::AsName,
    Which::Stage,
    Which::Unit,
    Which::Host,
    Which::Id,
];

fn fit(record: &mut Record, limit: usize) {
    for _ in 0..24 {
        let len = serde_json::to_vec(record).map_or(0, |bytes| bytes.len());
        if len <= limit {
            return;
        }
        let over = len - limit;
        if !record.stages.is_empty() {
            record.stages.clear();
            continue;
        }
        if shrink(record, &PREFERRED, over) || shrink(record, &EXTRA, over) {
            continue;
        }
        record.label.clear();
        record.detail = None;
        record.stage_note = None;
        record.error = None;
        record.device = None;
        record.out = None;
        return;
    }
}

fn shrink(record: &mut Record, group: &[Which], over: usize) -> bool {
    let mut best: Option<Which> = None;
    let mut best_len = 0;
    for which in group {
        let len = text(record, *which).map(str::len).unwrap_or(0);
        if len > best_len {
            best_len = len;
            best = Some(*which);
        }
    }
    best.and_then(|which| text_mut(record, which))
        .is_some_and(|text| shorten(text, over))
}

fn shorten(text: &mut String, over: usize) -> bool {
    let before = text.len();
    let keep = text.floor_char_boundary(before.saturating_sub(over.max(1)));
    text.truncate(keep);
    text.len() < before
}

fn text(record: &Record, which: Which) -> Option<&str> {
    match which {
        Which::Label => Some(record.label.as_str()),
        Which::Detail => record.detail.as_deref(),
        Which::StageNote => record.stage_note.as_deref(),
        Which::Error => record.error.as_deref(),
        Which::Device => record.device.as_deref(),
        Which::Out => record.out.as_deref(),
        Which::App => record.app.as_deref(),
        Which::Invoker => record.invoker.as_deref(),
        Which::Parent => record.parent.as_deref(),
        Which::Tool => Some(record.tool.as_str()),
        Which::Host => Some(record.host.as_str()),
        Which::Id => Some(record.id.as_str()),
        Which::Stage => record.stage.as_deref(),
        Which::Unit => record
            .progress
            .as_ref()
            .map(|progress| progress.unit.as_str()),
        Which::AsName => record.pgpu.as_ref().map(|pgpu| pgpu.as_name.as_str()),
    }
}

fn text_mut(record: &mut Record, which: Which) -> Option<&mut String> {
    match which {
        Which::Label => Some(&mut record.label),
        Which::Detail => record.detail.as_mut(),
        Which::StageNote => record.stage_note.as_mut(),
        Which::Error => record.error.as_mut(),
        Which::Device => record.device.as_mut(),
        Which::Out => record.out.as_mut(),
        Which::App => record.app.as_mut(),
        Which::Invoker => record.invoker.as_mut(),
        Which::Parent => record.parent.as_mut(),
        Which::Tool => Some(&mut record.tool),
        Which::Host => Some(&mut record.host),
        Which::Id => Some(&mut record.id),
        Which::Stage => record.stage.as_mut(),
        Which::Unit => record.progress.as_mut().map(|progress| &mut progress.unit),
        Which::AsName => record.pgpu.as_mut().map(|pgpu| &mut pgpu.as_name),
    }
}

#[cfg(test)]
mod tests {
    use super::super::record::{Pgpu, Progress};
    use super::*;

    fn base() -> Record {
        Record {
            v: 1,
            id: "id-1".to_string(),
            host: "box".to_string(),
            pid: 42,
            pid_start: 7,
            tool: "prec".to_string(),
            app: Some("demo".to_string()),
            label: "intro".to_string(),
            detail: Some("1280x720".to_string()),
            invoker: Some("agent".to_string()),
            parent: Some("desk:1:2".to_string()),
            state: "running".to_string(),
            stage: Some("replay".to_string()),
            stages: vec!["record".to_string(), "replay".to_string()],
            stage_note: Some("dark".to_string()),
            progress: Some(Progress {
                done: 3,
                total: Some(9),
                unit: "frames".to_string(),
            }),
            rate: Some(1.5),
            pgpu: Some(Pgpu {
                pid: 9,
                as_name: "render".to_string(),
            }),
            device: Some("gpu0".to_string()),
            out: Some("out.png".to_string()),
            started: 1,
            updated: 2,
            ended: None,
            exit: None,
            error: Some("none".to_string()),
        }
    }

    type Set = fn(&mut Record, String);
    type Get = fn(&Record) -> Option<String>;

    const FIELDS: [(&str, Set, Get); 15] = [
        ("label", |r, s| r.label = s, |r| Some(r.label.clone())),
        ("detail", |r, s| r.detail = Some(s), |r| r.detail.clone()),
        (
            "stage_note",
            |r, s| r.stage_note = Some(s),
            |r| r.stage_note.clone(),
        ),
        ("error", |r, s| r.error = Some(s), |r| r.error.clone()),
        ("device", |r, s| r.device = Some(s), |r| r.device.clone()),
        ("out", |r, s| r.out = Some(s), |r| r.out.clone()),
        ("app", |r, s| r.app = Some(s), |r| r.app.clone()),
        ("invoker", |r, s| r.invoker = Some(s), |r| r.invoker.clone()),
        ("parent", |r, s| r.parent = Some(s), |r| r.parent.clone()),
        ("tool", |r, s| r.tool = s, |r| Some(r.tool.clone())),
        ("host", |r, s| r.host = s, |r| Some(r.host.clone())),
        ("id", |r, s| r.id = s, |r| Some(r.id.clone())),
        ("stage", |r, s| r.stage = Some(s), |r| r.stage.clone()),
        (
            "unit",
            |r, s| r.progress.as_mut().unwrap().unit = s,
            |r| r.progress.as_ref().map(|p| p.unit.clone()),
        ),
        (
            "as",
            |r, s| r.pgpu.as_mut().unwrap().as_name = s,
            |r| r.pgpu.as_ref().map(|p| p.as_name.clone()),
        ),
    ];

    fn decode(bytes: &[u8]) -> Record {
        serde_json::from_slice(bytes).unwrap()
    }

    #[test]
    fn a_record_within_the_limit_is_written_byte_for_byte() {
        let rec = base();
        assert_eq!(encode(&rec), serde_json::to_vec(&rec).unwrap());
        let mut exact = base();
        let len = serde_json::to_vec(&exact).unwrap().len();
        exact.label.push_str(&"x".repeat(LIMIT - len));
        let bytes = encode(&exact);
        assert_eq!(bytes.len(), LIMIT);
        assert_eq!(decode(&bytes), exact);
    }

    #[test]
    fn each_field_alone_over_the_limit_is_cut_just_enough_and_nothing_else_moves() {
        for (index, (name, set, get)) in FIELDS.into_iter().enumerate() {
            let mut rec = base();
            rec.stages.clear();
            if index >= PREFERRED.len() {
                rec.label.clear();
                rec.detail = None;
                rec.stage_note = None;
                rec.error = None;
            }
            let long = "z".repeat(LIMIT + 500);
            set(&mut rec, long.clone());
            let bytes = encode(&rec);
            assert_eq!(bytes.len(), LIMIT, "{name}");
            let back = decode(&bytes);
            let got = get(&back).unwrap();
            assert!(
                !got.is_empty() && got.len() < long.len(),
                "{name}: {}",
                got.len()
            );
            assert!(long.starts_with(&got), "{name}");
            for (other, _, get_other) in FIELDS {
                if other != name {
                    assert_eq!(get_other(&back), get_other(&rec), "{name} moved {other}");
                }
            }
            assert_eq!(back.pid, rec.pid);
            assert_eq!(back.progress.as_ref().unwrap().done, 3);
            assert_eq!(back.rate, rec.rate);
        }
    }

    #[test]
    fn the_stages_list_goes_before_any_text_is_cut() {
        let mut rec = base();
        rec.stages = vec!["s".repeat(1_500); 4];
        rec.label = "l".repeat(1_000);
        let back = decode(&encode(&rec));
        assert!(back.stages.is_empty());
        assert_eq!(back.label, rec.label);
        assert_eq!(back.stage, rec.stage);
    }

    #[test]
    fn a_preferred_field_is_cut_before_a_larger_identity_field() {
        let mut rec = base();
        rec.stages.clear();
        rec.label = "l".repeat(1_500);
        rec.app = Some("a".repeat(3_000));
        let back = decode(&encode(&rec));
        assert_eq!(back.app, rec.app);
        assert!(back.label.len() < 1_500);
        assert!(rec.label.starts_with(&back.label));
    }

    #[test]
    fn the_largest_preferred_field_is_cut_first() {
        let mut rec = base();
        rec.stages.clear();
        rec.detail = Some("d".repeat(2_000));
        rec.error = Some("e".repeat(2_500));
        let back = decode(&encode(&rec));
        assert_eq!(back.detail, rec.detail);
        let error = back.error.unwrap();
        assert!(error.len() < 2_500 && !error.is_empty(), "{}", error.len());
    }

    #[test]
    fn text_that_json_escapes_still_fits_and_stays_a_prefix() {
        for unit in ["\"", "\\", "\u{1}", "é", "😀"] {
            let mut rec = base();
            rec.stages.clear();
            let long = unit.repeat(LIMIT);
            rec.detail = Some(long.clone());
            let bytes = encode(&rec);
            assert!(!bytes.is_empty(), "{unit:?}");
            assert!(bytes.len() <= LIMIT, "{unit:?}: {}", bytes.len());
            let got = decode(&bytes).detail.unwrap();
            assert!(long.starts_with(&got), "{unit:?}");
            assert_eq!(decode(&bytes).label, rec.label);
        }
    }

    #[test]
    fn a_record_that_cannot_fit_at_all_gives_no_bytes() {
        let rec = base();
        assert!(encode_within(&rec, 16).is_empty());
    }

    #[test]
    fn with_every_text_cut_and_still_over_the_optional_texts_are_dropped() {
        let mut rec = base();
        fit(&mut rec, 16);
        assert!(rec.stages.is_empty());
        assert!(rec.label.is_empty());
        assert_eq!(rec.detail, None);
        assert_eq!(rec.stage_note, None);
        assert_eq!(rec.error, None);
        assert_eq!(rec.device, None);
        assert_eq!(rec.out, None);
        assert_eq!(rec.tool, "");
        assert_eq!(rec.id, "");
        assert_eq!(rec.pid, 42);
        assert_eq!(rec.state, "running");
    }

    #[test]
    fn a_limit_the_record_meets_after_cutting_keeps_what_fits() {
        let rec = base();
        let full = serde_json::to_vec(&rec).unwrap().len();
        let bytes = encode_within(&rec, full - 3);
        assert!(!bytes.is_empty());
        assert!(bytes.len() <= full - 3);
        let back = decode(&bytes);
        assert!(back.stages.is_empty());
        assert_eq!(back.label, rec.label);
    }

    #[test]
    fn an_empty_text_is_never_reported_as_cut() {
        let mut empty = String::new();
        assert!(!shorten(&mut empty, 5));
        let mut short = "abc".to_string();
        assert!(shorten(&mut short, 10));
        assert_eq!(short, "");
        let mut one = "abc".to_string();
        assert!(shorten(&mut one, 0));
        assert_eq!(one, "ab");
        let mut wide = "aé".to_string();
        assert!(shorten(&mut wide, 1));
        assert_eq!(wide, "a");
    }

    #[test]
    fn no_text_left_to_cut_is_no_progress() {
        let mut rec = base();
        rec.label.clear();
        rec.detail = None;
        rec.stage_note = None;
        rec.error = Some(String::new());
        assert!(!shrink(&mut rec, &PREFERRED, 10));
        assert!(shrink(&mut rec, &EXTRA, 1));
    }
}
