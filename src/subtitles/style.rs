//! Replayer's readable-dialogue profile, based on Netflix's Chinese, English
//! and general timed-text guidance. This is not a delivery-compliance checker.
use super::Cue;
use crate::settings::Language;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub const MIN_DURATION: f64 = 5.0 / 6.0;
pub const MAX_DURATION: f64 = 7.0;
#[derive(Clone, Copy, Debug, Default)]
pub struct Quality {
    pub fast: usize,
    pub short: usize,
}

fn limit(language: Language) -> usize {
    if language == Language::Chinese {
        32
    } else {
        42
    }
}
fn units(text: &str, language: Language) -> usize {
    if language == Language::Chinese {
        text.width()
    } else {
        text.graphemes(true).count()
    }
}
fn reading_chars(text: &str, language: Language) -> f64 {
    if language == Language::Chinese {
        text.width() as f64 / 2.0
    } else {
        text.graphemes(true).filter(|g| *g != "\n").count() as f64
    }
}
pub fn prompt_rules(language: Language) -> &'static str {
    language.text(
        "Use natural concise Simplified Chinese translations. At most 16 full-width characters per line and two lines per subtitle. Do not use ordinary Chinese commas or full stops: use one space between clauses, and no trailing space. Preserve decimal points, abbreviations, enumeration commas, quotes, questions and exclamations; use full-width ？ and ！. Use … only for a real hesitation or trailing speech, not to link consecutive subtitles. Aim for no more than 9 Chinese characters per second. Break at phrases and do not omit meaningful dialogue.",
        "Use natural English, at most 42 characters per line and two lines per subtitle. Preserve normal sentence punctuation, decimal points, abbreviations and apostrophes. Use a single … for actual hesitation, never insert ellipses merely because a sentence continues in the next subtitle. Aim for no more than 20 characters per second. Break at phrase boundaries without separating articles/adjectives from nouns or splitting words. Do not omit meaningful dialogue.",
    )
}

pub fn normalize(text: &str, language: Language) -> String {
    let text = text.replace("...", "…").replace('⋯', "…");
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        let before = i.checked_sub(1).and_then(|j| chars.get(j)).copied();
        let after = chars.get(i + 1).copied();
        let numeric =
            before.is_some_and(|c| c.is_ascii_digit()) && after.is_some_and(|c| c.is_ascii_digit());
        let latin_dot = ch == '.'
            && (numeric
                || (before.is_some_and(|c| c.is_ascii_alphabetic())
                    && (after.is_some_and(|c| c.is_ascii_alphabetic())
                        || result.split_whitespace().last().is_some_and(|w| {
                            matches!(w, "Mr" | "Mrs" | "Ms" | "Dr" | "Prof" | "St" | "vs" | "etc")
                        }))));
        let mapped = if language == Language::Chinese {
            match ch {
                '。' | '，' => ' ',
                ',' if !numeric => ' ',
                '.' if !latin_dot => ' ',
                '?' => '？',
                '!' => '！',
                ':' if !numeric => '：',
                ';' => '；',
                _ => ch,
            }
        } else {
            ch
        };
        if mapped == '…' && result.ends_with('…') {
            continue;
        }
        if language == Language::Chinese
            && matches!(mapped, '？' | '！')
            && result.ends_with(['？', '！'])
        {
            continue;
        }
        result.push(if mapped.is_whitespace() { ' ' } else { mapped });
    }
    result
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(if language == Language::Chinese {
            &['、'][..]
        } else {
            &[]
        })
        .to_owned()
}

fn word_boundary(left: &str, right: &str) -> bool {
    !(left
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '\'')
        && right
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '\''))
}
fn split_cost(left: &str, right: &str, language: Language) -> f64 {
    let balance = (units(left, language) as f64 * 1.1 - units(right, language) as f64).abs();
    let mut cost = balance;
    if right.starts_with([
        '，', '。', '、', '？', '！', ',', '.', '?', '!', ')', '）', '”', '’',
    ]) || left.ends_with(['（', '(', '“', '‘'])
    {
        cost += 100.0;
    }
    if language == Language::English
        && left.split_whitespace().last().is_some_and(|w| {
            matches!(
                w.to_lowercase().as_str(),
                "a" | "an" | "the" | "to" | "of" | "and" | "my" | "your"
            )
        })
    {
        cost += 20.0;
    }
    if left.ends_with([',', ';', ':', '?', '!', '？', '！', '：', '；']) || right.starts_with('-')
    {
        cost -= 5.0;
    }
    cost
}

/// Each page has at most two explicit lines. View rendering must not auto-wrap.
fn pages(text: &str, language: Language) -> Vec<String> {
    let cap = limit(language);
    let mut rest = text.trim().to_owned();
    let mut result = Vec::new();
    while !rest.is_empty() {
        let graphemes: Vec<&str> = rest.graphemes(true).collect();
        let mut end = 0;
        let mut width = 0;
        while end < graphemes.len() && width + units(graphemes[end], language) <= cap * 2 {
            width += units(graphemes[end], language);
            end += 1;
        }
        if end == 0 {
            end = 1;
        }
        if end < graphemes.len() {
            let natural = (1..=end)
                .rev()
                .find(|&i| i == graphemes.len() || word_boundary(graphemes[i - 1], graphemes[i]));
            if let Some(natural) = natural {
                end = natural;
            }
        }
        let page = graphemes[..end].concat().trim().to_owned();
        if units(&page, language) <= cap {
            if !page.is_empty() {
                result.push(page);
            }
        } else {
            let parts: Vec<&str> = page.graphemes(true).collect();
            let mut best: Option<(f64, String)> = None;
            for i in 1..parts.len() {
                let left = parts[..i].concat();
                let right = parts[i..].concat();
                let left = left.trim();
                let right = right.trim();
                if left.is_empty()
                    || right.is_empty()
                    || units(left, language) > cap
                    || units(right, language) > cap
                {
                    continue;
                }
                let mut cost = split_cost(left, right, language);
                if !word_boundary(parts[i - 1], parts[i]) {
                    cost += 1000.0;
                }
                if best.as_ref().is_none_or(|(score, _)| cost < *score) {
                    best = Some((cost, format!("{left}\n{right}")));
                }
            }
            result.push(best.map(|(_, text)| text).unwrap_or(page));
        }
        rest = graphemes[end..].concat().trim().to_owned();
    }
    result
}

pub fn prepare(cues: Vec<Cue>, language: Language, boundary: f64) -> Vec<Cue> {
    let mut merged: Vec<Cue> = Vec::new();
    for mut cue in cues {
        cue.text = normalize(&cue.text, language);
        if cue.text.is_empty() {
            continue;
        }
        if let Some(previous) = merged.last_mut()
            && cue.start < previous.end
        {
            if previous.text != cue.text {
                previous.text.push(' ');
                previous.text.push_str(&cue.text);
            }
            previous.end = previous.end.max(cue.end);
            continue;
        }
        merged.push(cue);
    }
    let mut result = Vec::new();
    for (i, cue) in merged.iter().enumerate() {
        let parts = pages(&cue.text, language);
        let weights: Vec<f64> = parts
            .iter()
            .map(|p| reading_chars(p, language).max(1.0))
            .collect();
        let total: f64 = weights.iter().sum();
        let next = merged
            .get(i + 1)
            .map_or(boundary, |c| c.start)
            .min(boundary);
        let cps = if language == Language::Chinese {
            9.0
        } else {
            20.0
        };
        let readable = reading_chars(&cue.text, language) / cps;
        let desired = (cue.start + MIN_DURATION.max(readable))
            .max(cue.end)
            .min(cue.end + 0.5)
            .min(next);
        let mut position = cue.start;
        for (text, weight) in parts.into_iter().zip(weights) {
            let allocation = (desired - cue.start) * weight / total;
            let end = (position + allocation).min(position + MAX_DURATION);
            if end > position {
                result.push(Cue {
                    start: position,
                    end,
                    text,
                });
            }
            position += allocation;
        }
    }
    result
}
pub fn quality(cues: &[Cue], language: Language) -> Quality {
    let cps = if language == Language::Chinese {
        9.0
    } else {
        20.0
    };
    Quality {
        fast: cues
            .iter()
            .filter(|c| reading_chars(&c.text, language) / (c.end - c.start) > cps + 0.01)
            .count(),
        short: cues
            .iter()
            .filter(|c| c.end - c.start < MIN_DURATION - 0.001)
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn punctuation_is_language_specific_and_preserves_numbers() {
        assert_eq!(
            normalize(
                "你好，世界。真的吗?? 等等... 3.14，Dr. Smith。",
                Language::Chinese
            ),
            "你好 世界 真的吗？ 等等… 3.14 Dr. Smith"
        );
        assert_eq!(
            normalize("Hello, world. Really? 3.14...", Language::English),
            "Hello, world. Really? 3.14…"
        );
    }
    #[test]
    fn long_dialogue_preserves_text_without_third_lines_or_overlaps() {
        for language in [Language::Chinese, Language::English] {
            let text = language.text("这是一段非常长的中文字幕用于验证不会出现超过两行的情况并且不会丢失原本的对白内容", "This is a long subtitle which must remain readable and preserve every spoken word while being split into two-line pages.");
            let cues = prepare(
                vec![Cue {
                    start: 0.0,
                    end: 6.0,
                    text: text.into(),
                }],
                language,
                8.0,
            );
            for c in &cues {
                assert!(c.text.lines().count() <= 2);
                assert!(
                    c.text
                        .lines()
                        .all(|l| units(l, language) <= limit(language))
                );
            }
            assert!(cues.windows(2).all(|c| c[0].end <= c[1].start));
            let squash = |s: String| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(
                squash(cues.iter().map(|c| c.text.clone()).collect()),
                squash(normalize(text, language))
            );
        }
    }
    #[test]
    fn timing_is_bounded_and_unfixable_speed_is_reported() {
        let cues = prepare(
            vec![
                Cue {
                    start: 0.0,
                    end: 0.1,
                    text: "这是一个很短的片段".into(),
                },
                Cue {
                    start: 0.2,
                    end: 12.0,
                    text: "好".into(),
                },
            ],
            Language::Chinese,
            12.0,
        );
        assert!(cues[0].end <= 0.2);
        assert!(cues[1].end - cues[1].start <= 7.0);
        assert!(quality(&cues, Language::Chinese).fast > 0);
    }
}
