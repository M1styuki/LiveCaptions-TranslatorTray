//! Read-only source-compatibility probe; JSONL input/output, no UI or network.
use captions_core::{segment, text};
use serde_json::{Value, json};
use std::io::{self, BufRead};

fn main() {
    let mut gate = segment::SyncGate::default();
    let mut segmenter = segment::Segmenter::default();
    let mut last = String::new();
    let mut revision = 0;
    for line in io::stdin().lock().lines() {
        let input: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if input["reset"].as_bool().unwrap_or(false) {
            gate = segment::SyncGate::default();
            segmenter = segment::Segmenter::default();
            last.clear();
            revision = 0;
        }
        let raw = input["text"].as_str().unwrap();
        let display = input["display"].as_u64().unwrap() as usize;
        let contexts = input["contexts"].as_u64().unwrap() as usize;
        let max_sync = input["max_sync"].as_u64().unwrap() as u8;
        let Some(extracted) = segment::extract(raw, display, contexts) else {
            println!("{}", json!({"skipped":true}));
            continue;
        };
        if extracted.translation != last {
            revision += 1;
            last.clone_from(&extracted.translation);
        }
        let submit = gate.observe(&extracted.translation, 1, revision, max_sync);
        let observed = segmenter.observe(raw, max_sync, display, contexts).unwrap();
        let second = segment::extract(&text::normalize_caption(raw), display, contexts).unwrap();
        println!(
            "{}",
            json!({
                "normalized":text::normalize_caption(raw), "latest":extracted.latest,
                "translation":extracted.translation, "overlay":extracted.overlay,
                "display":extracted.display, "clear_context":extracted.clear_context,
                "submit":submit,"segment_submit":observed.submit,
                "sentence":observed.sentence.id,"revision":observed.sentence.revision,
                "double_normalized_translation":second.translation,
            })
        );
    }
}
