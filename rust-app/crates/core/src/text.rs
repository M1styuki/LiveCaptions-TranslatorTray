use std::collections::BTreeMap;

pub const EOS: &str = ".?!。？！";
pub const COMMA: &str = ",，、—\n";
pub const SHORT: usize = 10;
pub const MEDIUM: usize = 40;
pub const LONG: usize = 160;
pub const VERY_LONG: usize = 220;
pub const MARKER: &str = "🔤";
pub fn is_eos(ch: char) -> bool {
    EOS.contains(ch)
}
pub fn is_cj(ch: char) -> bool {
    matches!(ch as u32,0x4e00..=0x9fff|0x3400..=0x4dbf|0x3000..=0x303f|0x3040..=0x309f|0x30a0..=0x30ff|0x31f0..=0x31ff|0x3200..=0x32ff|0x3300..=0x33ff)
}

pub fn replace_newlines(text: &str, threshold: usize) -> String {
    let parts: Vec<_> = text
        .split('\n')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let mut result = String::with_capacity(text.len());
    for (index, part) in parts.iter().enumerate() {
        result.push_str(part);
        if index + 1 != parts.len() {
            let cj = part.chars().next_back().is_some_and(is_cj);
            result.push_str(match (part.len() >= threshold, cj) {
                (true, true) => "。",
                (true, false) => ". ",
                (false, true) => "——",
                (false, false) => "—",
            });
        }
    }
    result
}

fn acronym_pass(text: &str, with_words: bool) -> String {
    let chars: Vec<_> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let first = chars[index];
        if first.is_ascii_uppercase() {
            let mut dot = index + 1;
            while dot < chars.len() && chars[dot].is_whitespace() {
                dot += 1;
            }
            if chars.get(dot) == Some(&'.') {
                let mut second = dot + 1;
                while second < chars.len() && chars[second].is_whitespace() {
                    second += 1;
                }
                if chars.get(second).is_some_and(char::is_ascii_uppercase)
                    && chars.get(second + 1).is_some_and(char::is_ascii_alphabetic) == with_words
                {
                    result.push(first);
                    if with_words {
                        result.push(' ');
                    }
                    result.push(chars[second]);
                    index = second + 1;
                    continue;
                }
            }
        }
        result.push(first);
        index += 1;
    }
    result
}

fn punctuation_pass(text: &str, marks: &str, space: bool) -> String {
    let chars: Vec<_> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let mut punctuation = index;
        while punctuation < chars.len() && chars[punctuation].is_whitespace() {
            punctuation += 1;
        }
        if punctuation < chars.len() && marks.contains(chars[punctuation]) {
            result.push(chars[punctuation]);
            if space {
                result.push(' ');
            }
            index = punctuation + 1;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
        } else {
            result.push(chars[index]);
            index += 1;
        }
    }
    result
}

pub fn normalize_caption(text: &str) -> String {
    let text = acronym_pass(text, false);
    let text = acronym_pass(&text, true);
    let text = punctuation_pass(&text, ".!?,", true);
    let text = punctuation_pass(&text, "。！？，、", false);
    replace_newlines(&text, MEDIUM)
}

pub fn shorten_display_sentence(mut text: &str, max_bytes: usize) -> &str {
    while text.len() >= max_bytes {
        let Some((position, mark)) = text
            .char_indices()
            .find(|(_, ch)| is_eos(*ch) || COMMA.contains(*ch))
        else {
            break;
        };
        let start = position + mark.len_utf8();
        if start == text.len() {
            break;
        }
        text = &text[start..];
    }
    text
}

fn similarity_units(left: &[u16], right: &[u16]) -> f64 {
    if left.starts_with(right) || right.starts_with(left) {
        return 1.0;
    }
    let (short, long) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    let mut previous: Vec<_> = (0..=short.len()).collect();
    let mut current = vec![0; short.len() + 1];
    for (row, right) in long.iter().enumerate() {
        current[0] = row + 1;
        for (column, left) in short.iter().enumerate() {
            current[column + 1] = (current[column] + 1)
                .min(previous[column + 1] + 1)
                .min(previous[column] + usize::from(left != right));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    1.0 - previous[short.len()] as f64 / long.len() as f64
}
pub fn similarity(left: &str, right: &str) -> f64 {
    similarity_units(
        &left.encode_utf16().collect::<Vec<_>>(),
        &right.encode_utf16().collect::<Vec<_>>(),
    )
}
pub fn is_revision(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return false;
    }
    let left: Vec<_> = left.encode_utf16().collect();
    let right: Vec<_> = right.encode_utf16().collect();
    let length = left.len().min(right.len());
    similarity_units(&left[..length], &right[..length]) > 0.6
}

pub fn strip_notice(text: &str) -> &str {
    if text.starts_with('[') {
        let first_line = text.split('\n').next().unwrap_or(text);
        if let Some(position) = first_line.rfind("] ")
            && position > 1
        {
            return &text[position + 2..];
        }
    }
    text
}
pub fn extract_marked(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once(MARKER)?;
    let (current, _) = rest.split_once(MARKER)?;
    let current = current.trim();
    (!current.is_empty()).then_some(current)
}
pub fn clean_translation(text: &str) -> Option<String> {
    let mut cleaned = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        let after = start + "<think>".len();
        let Some(end) = rest[after..].find("</think>") else {
            break;
        };
        cleaned.push_str(&rest[..start]);
        rest = &rest[after + end + "</think>".len()..];
    }
    cleaned.push_str(rest);
    let cleaned = cleaned.replace(MARKER, "");
    let cleaned = strip_notice(cleaned.trim()).trim();
    (!cleaned.is_empty()).then(|| cleaned.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub source: String,
    pub translation: String,
}
#[derive(Default)]
pub struct Contexts {
    entries: BTreeMap<u64, (u64, Context)>,
}
impl Contexts {
    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Sentence order, rather than HTTP completion order, defines context order.
    pub fn upsert(&mut self, sentence: u64, revision: u64, context: Context) {
        if context.translation.trim().is_empty()
            || context.translation == "N/A"
            || context.translation.contains("[ERROR]")
            || context.translation.contains("[WARNING]")
        {
            return;
        }
        if self
            .entries
            .get(&sentence)
            .is_some_and(|(previous, _)| *previous >= revision)
        {
            return;
        }
        self.entries.insert(sentence, (revision, context));
        if self.entries.len() > 10 {
            self.entries.pop_first();
        }
    }
    pub fn recent(&self, count: usize) -> impl Iterator<Item = &Context> {
        self.entries
            .values()
            .skip(self.entries.len().saturating_sub(count))
            .map(|(_, context)| context)
    }
    pub fn before(&self, sentence: u64, count: usize) -> impl Iterator<Item = &Context> {
        let available = self.entries.range(..sentence).count();
        self.entries
            .range(..sentence)
            .skip(available.saturating_sub(count))
            .map(|(_, (_, context))| context)
    }
    pub fn previous_text(&self, count: usize, translation: bool) -> String {
        Self::join(self.recent(count), translation)
    }
    pub fn previous_text_before(&self, sentence: u64, count: usize, translation: bool) -> String {
        Self::join(self.before(sentence, count), translation)
    }
    fn join<'a>(entries: impl Iterator<Item = &'a Context>, translation: bool) -> String {
        let mut result = String::new();
        for entry in entries {
            if let Some(last) = result.chars().next_back() {
                if !is_eos(last) {
                    result.push_str(if is_cj(last) { "。" } else { ". " });
                } else if !is_cj(last) {
                    result.push(' ');
                }
            }
            result.push_str(strip_notice(if translation {
                &entry.translation
            } else {
                &entry.source
            }));
        }
        if let Some(last) = result.chars().next_back()
            && !is_eos(last)
        {
            result.push_str(if is_cj(last) { "。" } else { "." });
        }
        if result
            .chars()
            .next_back()
            .is_some_and(|last| last.is_ascii())
        {
            result.push(' ');
        }
        result
    }
    pub fn non_llm_input(&self, current: &str, count: usize, aware: bool) -> String {
        self.non_llm_input_before(u64::MAX, current, count, aware)
    }
    pub fn non_llm_input_before(
        &self,
        sentence: u64,
        current: &str,
        count: usize,
        aware: bool,
    ) -> String {
        if !aware {
            return current.into();
        }
        format!(
            "{} {} {} {}",
            self.previous_text_before(sentence, count, false),
            MARKER,
            current,
            MARKER
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_contract_newlines_and_utf8_thresholds() {
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../docs/handoff/Source-Contract.json"
        ))
        .unwrap();
        for case in contract["rewriteTextCases"].as_array().unwrap() {
            assert_eq!(
                replace_newlines(case["input"].as_str().unwrap(), 40),
                case["expected"].as_str().unwrap(),
                "{}",
                case["id"]
            );
        }
        assert_eq!(
            replace_newlines(&format!("{}\nb", "A".repeat(40)), 40),
            format!("{}. b", "A".repeat(40))
        );
        assert_eq!(
            replace_newlines(&format!("{}\n文", "中".repeat(14)), 40),
            format!("{}。文", "中".repeat(14))
        );
        assert_eq!(shorten_display_sentence("abcdefgh", 5), "abcdefgh");
        assert_eq!(shorten_display_sentence("abc,def", 5), "def");
        assert_eq!(shorten_display_sentence("甲，乙，丙", 7), "丙");
    }
    #[test]
    fn punctuation_and_abbreviations_keep_the_reference_pass_order() {
        assert_eq!(normalize_caption("A . I"), "AI");
        assert_eq!(normalize_caption("A . Intelligent"), "A Intelligent");
        assert_eq!(normalize_caption("U.S.A"), "US. A");
        assert_eq!(
            normalize_caption("Hello  ,  world\n你好 ， 世界！"),
            "Hello, world—你好，世界！"
        );
        assert_eq!(normalize_caption("Hi.."), "Hi. .");
    }
    #[test]
    fn utf16_revision_rule_includes_surrogate_pairs_and_strict_threshold() {
        assert_eq!(similarity("abc", "abcd"), 1.0);
        assert_eq!(similarity("abc", "xyz"), 0.0);
        assert!(is_revision("😀a", "😃a"));
        assert!(!is_revision("abcde", "abXXe"));
        assert!(!is_revision("", "abc"));
    }
    #[test]
    fn marker_extraction_precedes_cleanup_and_multiline_thinking_is_removed() {
        assert_eq!(extract_marked("history 🔤 你好 🔤 trailing"), Some("你好"));
        assert_eq!(extract_marked("no markers"), None);
        assert_eq!(extract_marked("🔤 🔤"), None);
        assert_eq!(
            clean_translation("<think>line1\nline2</think>🔤 你好 🔤"),
            Some("你好".into())
        );
        assert_eq!(
            clean_translation("<think>first</think><think>second</think>"),
            None
        );
        assert_eq!(clean_translation("[123 ms] 你好"), Some("你好".into()));
        assert_eq!(clean_translation("[a] [b] hello"), Some("hello".into()));
        assert_eq!(
            clean_translation("<think>unclosed"),
            Some("<think>unclosed".into())
        );
    }
    #[test]
    fn contexts_are_bounded_successes_in_chronological_order() {
        let mut contexts = Contexts::default();
        contexts.upsert(
            1,
            1,
            Context {
                source: "First".into(),
                translation: "第一".into(),
            },
        );
        contexts.upsert(
            2,
            1,
            Context {
                source: "Second.".into(),
                translation: "第二".into(),
            },
        );
        contexts.upsert(
            3,
            1,
            Context {
                source: "Skipped".into(),
                translation: "N/A".into(),
            },
        );
        assert_eq!(contexts.previous_text(2, false), "First. Second. ");
        assert_eq!(contexts.previous_text(2, true), "第一。第二。");
        assert_eq!(contexts.non_llm_input("Current", 2, false), "Current");
        assert_eq!(
            contexts.non_llm_input("Current", 2, true),
            "First. Second.  🔤 Current 🔤"
        );
        for index in 0..30 {
            contexts.upsert(
                index + 3,
                1,
                Context {
                    source: format!("source {index}"),
                    translation: format!("译文{index}"),
                },
            );
        }
        assert_eq!(contexts.len(), 10);
        assert_eq!(contexts.recent(2).next().unwrap().source, "source 28");
    }
    #[test]
    fn out_of_order_replies_and_revisions_do_not_reorder_or_duplicate_context() {
        let mut contexts = Contexts::default();
        for (sentence, revision, source, translation) in [
            (2, 1, "Second.", "第二。"),
            (1, 1, "First.", "第一。"),
            (1, 2, "First revised.", "修订第一。"),
            (1, 1, "Old.", "旧。"),
            (3, 1, "Third.", "第三。"),
        ] {
            contexts.upsert(
                sentence,
                revision,
                Context {
                    source: source.into(),
                    translation: translation.into(),
                },
            );
        }
        assert_eq!(contexts.len(), 3);
        assert_eq!(
            contexts.previous_text(3, false),
            "First revised. Second. Third. "
        );
        assert_eq!(
            contexts.previous_text_before(2, 10, false),
            "First revised. "
        );
        assert_eq!(
            contexts.non_llm_input_before(2, "Second revised.", 10, true),
            "First revised.  🔤 Second revised. 🔤"
        );
        assert_eq!(contexts.before(1, 10).count(), 0);
    }
}
