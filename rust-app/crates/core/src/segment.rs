use crate::text::{self, is_eos};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extracted {
    pub latest: String,
    pub translation: String,
    pub overlay: String,
    pub display: String,
    pub boundary_prefix: String,
    pub clear_context: bool,
}
fn previous_boundary(text: &str, before: usize) -> Option<(usize, char)> {
    text[..before]
        .char_indices()
        .rev()
        .find(|(_, ch)| is_eos(*ch))
}
pub fn extract(full: &str, display_sentences: usize, contexts: usize) -> Option<Extracted> {
    let full = text::normalize_caption(full);
    if full.is_empty() {
        return None;
    }
    let final_character = full.char_indices().next_back().unwrap();
    let mut boundary = previous_boundary(
        &full,
        if is_eos(final_character.1) {
            final_character.0
        } else {
            full.len()
        },
    );
    let start =
        |boundary: Option<(usize, char)>| boundary.map_or(0, |(index, ch)| index + ch.len_utf8());
    if boundary.is_some_and(|(position, _)| position > 0)
        && full.len() - start(boundary) < text::SHORT
    {
        boundary = previous_boundary(&full, boundary.unwrap().0);
    }
    let latest = &full[start(boundary)..];
    let translation_end = latest
        .char_indices()
        .rev()
        .find(|(_, ch)| is_eos(*ch))
        .map_or(latest.len(), |(position, ch)| position + ch.len_utf8());
    let mut overlay_boundary = boundary;
    for _ in 0..display_sentences.min(contexts) {
        if let Some((position, _)) = overlay_boundary
            && position > 0
        {
            overlay_boundary = previous_boundary(&full, position);
        } else {
            break;
        }
    }
    Some(Extracted {
        translation: latest[..translation_end].into(),
        overlay: full[start(overlay_boundary)..].into(),
        display: text::shorten_display_sentence(latest, text::VERY_LONG).into(),
        boundary_prefix: full[..start(boundary)].into(),
        latest: latest.into(),
        clear_context: !full.chars().any(is_eos) && contexts > 0,
    })
}

#[derive(Default)]
pub struct SyncGate {
    last: String,
    idle: u16,
    sync: u16,
    submitted: Option<(u64, u64)>,
    sentence: Option<u64>,
}
impl SyncGate {
    /// Called at each successful active UIA poll, including unchanged text.
    pub fn observe(&mut self, text: &str, sentence: u64, revision: u64, max_sync: u8) -> bool {
        let changed = self.last != text || self.sentence != Some(sentence);
        self.observe_sample(
            text,
            sentence,
            revision,
            max_sync,
            u64::from(changed),
            if changed {
                0
            } else {
                self.idle.saturating_add(1)
            },
        )
    }
    /// UIA supplies counters with its latest snapshot. A temporarily busy UI
    /// must not turn the specified 50 successful polls into 50 UI wakeups.
    pub fn observe_sample(
        &mut self,
        text: &str,
        sentence: u64,
        revision: u64,
        max_sync: u8,
        changes: u64,
        unchanged: u16,
    ) -> bool {
        let changed = self.last != text || self.sentence != Some(sentence);
        self.idle = unchanged.min(51);
        if changed {
            self.sentence = Some(sentence);
            self.last = text.into();
            if text.chars().next_back().is_some_and(is_eos) {
                self.sync = 0;
                return self.submit(sentence, revision);
            }
        }
        if text.len() >= text::SHORT {
            self.sync = self.sync.saturating_add(changes.min(11) as u16);
        }
        if !text.is_empty() && (self.sync > u16::from(max_sync.min(10)) || self.idle >= 50) {
            self.sync = 0;
            return self.submit(sentence, revision);
        }
        false
    }
    fn submit(&mut self, sentence: u64, revision: u64) -> bool {
        if self.submitted == Some((sentence, revision)) {
            return false;
        }
        self.submitted = Some((sentence, revision));
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sentence {
    pub id: u64,
    pub revision: u64,
    pub source: String,
    pub complete: bool,
}
#[derive(Default)]
pub struct Segmenter {
    current: Option<Sentence>,
    tail_complete: bool,
    prefix: String,
    next_id: u64,
    gate: SyncGate,
    explicit_boundary: bool,
}
pub struct Observation {
    pub text: Extracted,
    pub sentence: Sentence,
    pub submit: bool,
    pub retained_boundary: bool,
}
impl Segmenter {
    /// A new translation session can replay the current sentence while keeping
    /// the boundary identity needed to exclude an explicitly deleted sentence.
    pub fn restart_sync(&mut self) -> Option<(u64, u64)> {
        self.gate = SyncGate::default();
        // Configuration/session changes allocate a fresh history identity,
        // while retaining the physical boundary used by deletion suppression.
        if let Some(current) = &mut self.current {
            let previous = current.id;
            self.next_id += 1;
            current.id = self.next_id;
            current.revision = 1;
            Some((previous, current.id))
        } else {
            None
        }
    }
    pub fn force_next_boundary(&mut self) {
        self.explicit_boundary = true;
    }
    pub fn observe(
        &mut self,
        raw: &str,
        max_sync: u8,
        display_sentences: usize,
        contexts: usize,
    ) -> Option<Observation> {
        self.observe_impl(raw, max_sync, display_sentences, contexts, None)
    }
    pub fn observe_sample(
        &mut self,
        raw: &str,
        max_sync: u8,
        display_sentences: usize,
        contexts: usize,
        changes: u64,
        unchanged: u16,
    ) -> Option<Observation> {
        self.observe_impl(
            raw,
            max_sync,
            display_sentences,
            contexts,
            Some((changes, unchanged)),
        )
    }
    fn observe_impl(
        &mut self,
        raw: &str,
        max_sync: u8,
        display_sentences: usize,
        contexts: usize,
        sample: Option<(u64, u16)>,
    ) -> Option<Observation> {
        let Some(text) = extract(raw, display_sentences, contexts) else {
            self.reconfigure();
            return None;
        };
        // Revisions to an earlier sentence can change the prefix text without
        // introducing a boundary. Require an additional EOS in that prefix.
        let retained_boundary = self.tail_complete
            && self.current.as_ref().is_some_and(|current| {
                // A previously current complete sentence retained immediately
                // before the new tail is an observed rolling boundary. If it was
                // already there, an older-prefix edit is not another boundary.
                current.complete
                    && has_sentence_tail(&text.boundary_prefix, &current.source)
                    && !has_sentence_tail(&self.prefix, &current.source)
            });
        let new_boundary = self.explicit_boundary
            || text
                .boundary_prefix
                .chars()
                .filter(|ch| is_eos(*ch))
                .count()
                > self.prefix.chars().filter(|ch| is_eos(*ch)).count()
            || retained_boundary;
        self.explicit_boundary = false;
        self.prefix.clone_from(&text.boundary_prefix);
        let sentence = match &self.current {
            Some(current) if !new_boundary && current.source == text.translation => current.clone(),
            Some(current)
                if !new_boundary && text::is_revision(&current.source, &text.translation) =>
            {
                Sentence {
                    id: current.id,
                    revision: current.revision + 1,
                    source: text.translation.clone(),
                    complete: text.translation.chars().next_back().is_some_and(is_eos),
                }
            }
            _ => {
                self.next_id += 1;
                Sentence {
                    id: self.next_id,
                    revision: 1,
                    source: text.translation.clone(),
                    complete: text.translation.chars().next_back().is_some_and(is_eos),
                }
            }
        };
        let submit = if let Some((changes, unchanged)) = sample {
            self.gate.observe_sample(
                &sentence.source,
                sentence.id,
                sentence.revision,
                max_sync,
                changes,
                unchanged,
            )
        } else {
            self.gate
                .observe(&sentence.source, sentence.id, sentence.revision, max_sync)
        };
        self.current = Some(sentence.clone());
        self.tail_complete = text.latest.chars().next_back().is_some_and(is_eos);
        Some(Observation {
            text,
            sentence,
            submit,
            retained_boundary,
        })
    }
    pub fn reconfigure(&mut self) {
        self.current = None;
        self.tail_complete = false;
        self.prefix.clear();
        self.gate = SyncGate::default();
        self.explicit_boundary = false;
    }
}

fn has_sentence_tail(prefix: &str, source: &str) -> bool {
    prefix
        .trim_end()
        .strip_suffix(source.trim())
        .is_some_and(|before| {
            before.trim_end().is_empty()
                || before.trim_end().chars().next_back().is_some_and(is_eos)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reformatting_context_before_repeated_speech_does_not_allocate_another_boundary() {
        let mut segmenter = Segmenter::default();
        segmenter
            .observe("Earlier number two. This sentence is repeated.", 3, 0, 0)
            .unwrap();
        let repeated = segmenter
            .observe(
                "Earlier number two. This sentence is repeated. This sentence is repeated.",
                3,
                0,
                0,
            )
            .unwrap();
        let corrected = segmenter
            .observe(
                "Earlier number 2. This sentence is repeated. This sentence is repeated.",
                3,
                0,
                0,
            )
            .unwrap();
        assert_eq!(repeated.sentence.id, corrected.sentence.id);
        assert!(!corrected.retained_boundary);
        assert!(!corrected.submit);
    }
    #[test]
    fn one_accumulated_snapshot_extracts_only_the_original_latest_sentence() {
        let observation = Segmenter::default().observe(
            "An earlier completed sentence. Another historical completed sentence. The actual current sentence.", 3, 0, 0).unwrap();
        assert_eq!(observation.sentence.source, " The actual current sentence.");
        assert!(observation.submit);
    }
    #[test]
    fn four_changed_incomplete_snapshots_trigger_once_at_count_four() {
        let mut segmenter = Segmenter::default();
        for index in 1..=4 {
            let value = format!("Incomplete sentence revision {index}");
            let observation = segmenter.observe(&value, 3, 1, 0).unwrap();
            assert_eq!(observation.submit, index == 4);
        }
    }
    #[test]
    fn unchanged_snapshot_stability_fires_only_at_poll_fifty() {
        let mut gate = SyncGate::default();
        assert!(!gate.observe("short", 1, 1, 3));
        for poll in 1..=51 {
            assert_eq!(gate.observe("short", 1, 1, 3), poll == 50);
        }
        let mut gate = SyncGate::default();
        assert!(gate.observe("Complete.", 1, 1, 3));
        for _ in 0..100 {
            assert!(!gate.observe("Complete.", 1, 1, 3));
        }
    }
    #[test]
    fn coalesced_capture_counters_preserve_change_and_stability_thresholds() {
        let mut segmenter = Segmenter::default();
        let observation = segmenter
            .observe_sample("Incomplete sentence revision four", 3, 1, 0, 4, 0)
            .unwrap();
        assert!(observation.submit);
        let mut segmenter = Segmenter::default();
        assert!(
            !segmenter
                .observe_sample("short", 3, 1, 0, 1, 0)
                .unwrap()
                .submit
        );
        assert!(
            !segmenter
                .observe_sample("short", 3, 1, 0, 0, 49)
                .unwrap()
                .submit
        );
        assert!(
            segmenter
                .observe_sample("short", 3, 1, 0, 0, 51)
                .unwrap()
                .submit
        );
        assert!(
            !segmenter
                .observe_sample("short", 3, 1, 0, 0, 51)
                .unwrap()
                .submit
        );
    }
    #[test]
    fn explicit_new_boundary_never_overwrites_a_similar_complete_sentence() {
        let mut segmenter = Segmenter::default();
        let first = segmenter
            .observe("The first complete sentence.", 3, 1, 0)
            .unwrap();
        let second = segmenter
            .observe(
                "The first complete sentence. The first complete sentence again.",
                3,
                1,
                1,
            )
            .unwrap();
        assert_ne!(first.sentence.id, second.sentence.id);
        assert!(second.submit);
        assert_eq!(
            second.text.overlay,
            "The first complete sentence. The first complete sentence again."
        );
        let revised = segmenter
            .observe(
                "The first complete sentence. The first complete sentence again!",
                3,
                1,
                1,
            )
            .unwrap();
        assert_eq!(revised.sentence.id, second.sentence.id);
        assert!(revised.sentence.revision > second.sentence.revision);
    }
    #[test]
    fn short_tail_is_merged_for_display_without_translating_partial_tail() {
        let text = extract("A long complete sentence. Hi", 1, 1).unwrap();
        assert_eq!(text.latest, "A long complete sentence. Hi");
        assert_eq!(text.translation, "A long complete sentence.");
        assert!(extract("no punctuation", 1, 1).unwrap().clear_context);
        assert!(extract(" \n ", 1, 0).is_none());
    }
    #[test]
    fn repeated_identical_complete_sentences_still_get_distinct_ids() {
        let mut segmenter = Segmenter::default();
        let mut previous = 0;
        for count in 1..=4 {
            let observation = segmenter
                .observe(&"This is repeated. ".repeat(count), 3, 1, 1)
                .unwrap();
            assert!(observation.sentence.id > previous);
            assert!(observation.submit);
            previous = observation.sentence.id;
        }
        segmenter.reconfigure();
        assert!(
            segmenter
                .observe("This is repeated.", 3, 1, 0)
                .unwrap()
                .sentence
                .id
                > previous
        );
    }
    #[test]
    fn revising_earlier_context_does_not_duplicate_the_current_sentence() {
        let mut segmenter = Segmenter::default();
        let first = segmenter
            .observe("Earlier sentence. Current complete sentence.", 3, 1, 1)
            .unwrap();
        let revised_prefix = segmenter
            .observe(
                "An earlier corrected sentence. Current complete sentence.",
                3,
                1,
                1,
            )
            .unwrap();
        assert_eq!(first.sentence.id, revised_prefix.sentence.id);
        assert!(!revised_prefix.submit);
        segmenter.observe("", 3, 1, 1);
        let after_clear = segmenter
            .observe("Current complete sentence.", 3, 1, 1)
            .unwrap();
        assert!(after_clear.sentence.id > first.sentence.id);
        assert!(after_clear.submit);
    }
}
