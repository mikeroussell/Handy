use natural::phonetics::soundex;
use once_cell::sync::Lazy;
use regex::Regex;
use strsim::levenshtein;

use crate::settings::WordReplacement;

/// Builds an n-gram string by cleaning and concatenating words
///
/// Strips punctuation from each word, lowercases, and joins without spaces.
/// This allows matching "Charge B" against "ChargeBee".
fn build_ngram(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect::<Vec<_>>()
        .concat()
}

/// Finds the best matching custom word for a candidate string
///
/// Uses Levenshtein distance and Soundex phonetic matching to find
/// the best match above the given threshold.
///
/// # Arguments
/// * `candidate` - The cleaned/lowercased candidate string to match
/// * `custom_words` - Original custom words (for returning the replacement)
/// * `custom_words_nospace` - Custom words with spaces removed, lowercased (for comparison)
/// * `threshold` - Maximum similarity score to accept
///
/// # Returns
/// The best matching custom word and its score, if any match was found
fn find_best_match<'a>(
    candidate: &str,
    custom_words: &'a [String],
    custom_words_nospace: &[String],
    threshold: f64,
) -> Option<(&'a String, f64)> {
    if candidate.is_empty() || candidate.len() > 50 {
        return None;
    }

    let mut best_match: Option<&String> = None;
    let mut best_score = f64::MAX;

    for (i, custom_word_nospace) in custom_words_nospace.iter().enumerate() {
        // Skip if lengths are too different (optimization + prevents over-matching)
        // Use percentage-based check: max 25% length difference (prevents n-grams from
        // matching significantly shorter custom words, e.g., "openaigpt" vs "openai")
        let len_diff = (candidate.len() as i32 - custom_word_nospace.len() as i32).abs() as f64;
        let max_len = candidate.len().max(custom_word_nospace.len()) as f64;
        let max_allowed_diff = (max_len * 0.25).max(2.0); // At least 2 chars difference allowed
        if len_diff > max_allowed_diff {
            continue;
        }

        // Calculate Levenshtein distance (normalized by length)
        let levenshtein_dist = levenshtein(candidate, custom_word_nospace);
        let max_len = candidate.len().max(custom_word_nospace.len()) as f64;
        let levenshtein_score = if max_len > 0.0 {
            levenshtein_dist as f64 / max_len
        } else {
            1.0
        };

        // Calculate phonetic similarity using Soundex
        let phonetic_match = soundex(candidate, custom_word_nospace);

        // Combine scores: favor phonetic matches, but also consider string similarity
        let combined_score = if phonetic_match {
            levenshtein_score * 0.3 // Give significant boost to phonetic matches
        } else {
            levenshtein_score
        };

        // Accept if the score is good enough (configurable threshold)
        if combined_score < threshold && combined_score < best_score {
            best_match = Some(&custom_words[i]);
            best_score = combined_score;
        }
    }

    best_match.map(|m| (m, best_score))
}

/// Applies custom word corrections to transcribed text using fuzzy matching
///
/// This function corrects words in the input text by finding the best matches
/// from a list of custom words using a combination of:
/// - Levenshtein distance for string similarity
/// - Soundex phonetic matching for pronunciation similarity
/// - N-gram matching for multi-word speech artifacts (e.g., "Charge B" -> "ChargeBee")
///
/// # Arguments
/// * `text` - The input text to correct
/// * `custom_words` - List of custom words to match against
/// * `threshold` - Maximum similarity score to accept (0.0 = exact match, 1.0 = any match)
///
/// # Returns
/// The corrected text with custom words applied
pub fn apply_custom_words(text: &str, custom_words: &[String], threshold: f64) -> String {
    if custom_words.is_empty() {
        return text.to_string();
    }

    // Pre-compute lowercase versions to avoid repeated allocations
    let custom_words_lower: Vec<String> = custom_words.iter().map(|w| w.to_lowercase()).collect();

    // Pre-compute versions with spaces removed for n-gram comparison
    let custom_words_nospace: Vec<String> = custom_words_lower
        .iter()
        .map(|w| w.replace(' ', ""))
        .collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let mut matched = false;

        // Try n-grams from longest (3) to shortest (1) - greedy matching
        for n in (1..=3).rev() {
            if i + n > words.len() {
                continue;
            }

            let ngram_words = &words[i..i + n];
            let ngram = build_ngram(ngram_words);

            if let Some((replacement, _score)) =
                find_best_match(&ngram, custom_words, &custom_words_nospace, threshold)
            {
                // Extract punctuation from first and last words of the n-gram
                let (prefix, _) = extract_punctuation(ngram_words[0]);
                let (_, suffix) = extract_punctuation(ngram_words[n - 1]);

                // Preserve case from first word
                let corrected = preserve_case_pattern(ngram_words[0], replacement);

                result.push(format!("{}{}{}", prefix, corrected, suffix));
                i += n;
                matched = true;
                break;
            }
        }

        if !matched {
            result.push(words[i].to_string());
            i += 1;
        }
    }

    result.join(" ")
}

/// Preserves the case pattern of the original word when applying a replacement
fn preserve_case_pattern(original: &str, replacement: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) {
        replacement.to_uppercase()
    } else if original.chars().next().map_or(false, |c| c.is_uppercase()) {
        let mut chars: Vec<char> = replacement.chars().collect();
        if let Some(first_char) = chars.get_mut(0) {
            *first_char = first_char.to_uppercase().next().unwrap_or(*first_char);
        }
        chars.into_iter().collect()
    } else {
        replacement.to_string()
    }
}

/// Extracts punctuation prefix and suffix from a word
fn extract_punctuation(word: &str) -> (&str, &str) {
    let prefix_end = word.chars().take_while(|c| !c.is_alphanumeric()).count();
    let suffix_start = word
        .char_indices()
        .rev()
        .take_while(|(_, c)| !c.is_alphanumeric())
        .count();

    let prefix = if prefix_end > 0 {
        &word[..prefix_end]
    } else {
        ""
    };

    let suffix = if suffix_start > 0 {
        &word[word.len() - suffix_start..]
    } else {
        ""
    };

    (prefix, suffix)
}

/// Returns filler words appropriate for the given language code.
///
/// Some words like "um" and "ha" are real words in certain languages
/// (e.g., Portuguese "um" = "a/an", Spanish "ha" = "has"), so we only
/// include them as fillers for languages where they are truly fillers.
fn get_filler_words_for_language(lang: &str) -> &'static [&'static str] {
    let base_lang = lang.split(&['-', '_'][..]).next().unwrap_or(lang);

    match base_lang {
        "en" => &[
            "uh", "um", "uhm", "umm", "uhh", "uhhh", "ah", "hmm", "hm", "mmm", "mm", "mh", "eh",
            "ehh", "ha",
        ],
        "es" => &["ehm", "mmm", "hmm", "hm"],
        "pt" => &["ahm", "hmm", "mmm", "hm"],
        "fr" => &["euh", "hmm", "hm", "mmm"],
        "de" => &["äh", "ähm", "hmm", "hm", "mmm"],
        "it" => &["ehm", "hmm", "mmm", "hm"],
        "cs" => &["ehm", "hmm", "mmm", "hm"],
        "pl" => &["hmm", "mmm", "hm"],
        "tr" => &["hmm", "mmm", "hm"],
        "ru" => &["хм", "ммм", "hmm", "mmm"],
        "uk" => &["хм", "ммм", "hmm", "mmm"],
        "ar" => &["hmm", "mmm"],
        "ja" => &["hmm", "mmm"],
        "ko" => &["hmm", "mmm"],
        "vi" => &["hmm", "mmm", "hm"],
        "zh" => &["hmm", "mmm"],
        // Conservative universal fallback (no "um", "eh", "ha")
        _ => &[
            "uh", "uhm", "umm", "uhh", "uhhh", "ah", "hmm", "hm", "mmm", "mm", "mh", "ehh",
        ],
    }
}

static MULTI_SPACE_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s{2,}").unwrap());

/// Collapses repeated 1-2 letter words (3+ repetitions) to a single instance.
/// E.g., "wh wh wh wh" -> "wh", "I I I I" -> "I"
fn collapse_stutters(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < words.len() {
        let word = words[i];
        let word_lower = word.to_lowercase();

        // Only process 1-2 letter words
        if word_lower.len() <= 2 && word_lower.chars().all(|c| c.is_alphabetic()) {
            // Count consecutive repetitions (case-insensitive)
            let mut count = 1;
            while i + count < words.len() && words[i + count].to_lowercase() == word_lower {
                count += 1;
            }

            // If 3+ repetitions, collapse to single instance
            if count >= 3 {
                result.push(word);
                i += count;
            } else {
                result.push(word);
                i += 1;
            }
        } else {
            result.push(word);
            i += 1;
        }
    }

    result.join(" ")
}

/// Returns default correction markers for English.
/// These phrases signal that the speaker is correcting what they just said.
fn get_default_correction_markers() -> &'static [&'static str] {
    &[
        "i mean",
        "or rather",
        "well actually",
        "no wait",
        "let me rephrase",
        "what i meant was",
        "scratch that",
        "not that",
        "correction",
    ]
}

/// Splits text into clauses on punctuation boundaries (commas, dashes, semicolons, ellipses).
/// Returns trimmed clause strings.
fn split_into_clauses(text: &str) -> Vec<&str> {
    static CLAUSE_BOUNDARY: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?:\s*[,;\u{2014}]\s*|\s*--\s*|\s*\.{2,}\s*|\s*\u{2026}\s*)").unwrap()
    });

    let mut clauses = Vec::new();
    let mut last_end = 0;

    for m in CLAUSE_BOUNDARY.find_iter(text) {
        let clause = &text[last_end..m.start()];
        if !clause.trim().is_empty() {
            clauses.push(clause.trim());
        }
        last_end = m.end();
    }

    // Remaining text after last separator
    let remainder = &text[last_end..];
    if !remainder.trim().is_empty() {
        clauses.push(remainder.trim());
    }

    clauses
}

/// Pass 1: Detect correction markers and remove the preceding clause.
/// If punctuation boundaries exist, uses clause splitting. Otherwise, falls back
/// to treating the marker itself as a boundary.
fn collapse_marker_corrections(text: &str, markers: &[&str]) -> String {
    if markers.is_empty() {
        return text.to_string();
    }

    // Try punctuation-based clause splitting first
    let clauses = split_into_clauses(text);

    if clauses.len() >= 2 {
        // Process clause-by-clause
        let mut result_clauses: Vec<&str> = Vec::new();

        for clause in &clauses {
            let clause_lower = clause.to_lowercase();
            let mut found_marker = false;

            for marker in markers {
                if clause_lower.starts_with(marker) {
                    // This clause starts with a correction marker.
                    // Remove the preceding clause (if any) and strip the marker.
                    if !result_clauses.is_empty() {
                        result_clauses.pop();
                    }
                    let after_marker = clause[marker.len()..].trim();
                    if !after_marker.is_empty() {
                        result_clauses.push(after_marker);
                    }
                    found_marker = true;
                    break;
                }
            }

            if !found_marker {
                result_clauses.push(clause);
            }
        }

        return result_clauses.join(", ").trim().to_string();
    }

    // Fallback: no punctuation boundaries found. Search for markers mid-text.
    let text_lower = text.to_lowercase();
    // Try markers from longest to shortest to avoid partial matches
    let mut sorted_markers: Vec<&&str> = markers.iter().collect();
    sorted_markers.sort_by(|a, b| b.len().cmp(&a.len()));

    for marker in sorted_markers {
        if let Some(pos) = text_lower.find(marker) {
            if pos == 0 {
                // Marker at very start — no preceding clause to remove, skip
                continue;
            }
            let after_marker = text[pos + marker.len()..].trim();
            if !after_marker.is_empty() {
                return after_marker.to_string();
            }
        }
    }

    text.to_string()
}

/// Pass 2: Detect false starts where the speaker abandons a clause and restarts.
/// Criteria: adjacent clauses share opening 1-2 words and first clause is shorter.
fn collapse_false_starts(text: &str) -> String {
    let clauses = split_into_clauses(text);

    if clauses.len() < 2 {
        return text.to_string();
    }

    let mut result_clauses: Vec<&str> = Vec::new();

    for clause in &clauses {
        if let Some(prev) = result_clauses.last() {
            let prev_words: Vec<&str> = prev.split_whitespace().collect();
            let curr_words: Vec<&str> = clause.split_whitespace().collect();

            // Check if they share the same opening words (case-insensitive)
            // Short prev clauses (≤3 words) are likely fragments — single word match suffices.
            // Longer prev clauses need 2-word match to avoid false positives on emphasis.
            let shares_opener = if prev_words.is_empty() || curr_words.is_empty() {
                false
            } else if prev_words.len() <= 3 {
                // Short fragment: first word match is enough
                prev_words[0].to_lowercase() == curr_words[0].to_lowercase()
            } else if curr_words.len() >= 2 {
                // Longer clause: require first two words to match
                prev_words[0].to_lowercase() == curr_words[0].to_lowercase()
                    && prev_words[1].to_lowercase() == curr_words[1].to_lowercase()
            } else {
                prev_words[0].to_lowercase() == curr_words[0].to_lowercase()
            };

            // First clause must be shorter (incomplete thought restarted more fully)
            let first_is_shorter = prev_words.len() < curr_words.len();

            if shares_opener && first_is_shorter {
                // Replace the previous clause with this one
                result_clauses.pop();
                result_clauses.push(clause);
            } else {
                result_clauses.push(clause);
            }
        } else {
            result_clauses.push(clause);
        }
    }

    result_clauses.join(", ").trim().to_string()
}

/// Collapses self-corrections in transcribed text.
///
/// Applies two passes:
/// 1. Marker-based: detects correction phrases ("I mean", "scratch that", etc.)
///    and removes the preceding clause
/// 2. False-start: detects abandoned clauses restarted with the same opening words
///
/// # Arguments
/// * `text` - The transcription text (should already have fillers removed)
/// * `custom_correction_markers` - Optional custom marker list. `None` uses English defaults,
///   `Some(empty)` disables marker detection, `Some(list)` overrides defaults.
///
/// # Returns
/// The text with self-corrections collapsed
pub fn collapse_self_corrections(
    text: &str,
    custom_correction_markers: &Option<Vec<String>>,
) -> String {
    if text.is_empty() {
        return String::new();
    }

    // Build marker list
    let markers: Vec<&str> = match custom_correction_markers {
        Some(custom) => custom.iter().map(|s| s.as_str()).collect(),
        None => get_default_correction_markers().to_vec(),
    };

    // Pass 1: marker-based correction
    let after_markers = collapse_marker_corrections(text, &markers);

    // Pass 2: false-start detection
    collapse_false_starts(&after_markers)
}

/// Applies exact word-boundary replacements to transcription text.
///
/// Unlike `apply_custom_words` which uses fuzzy/phonetic matching,
/// this performs simple case-insensitive find-and-replace using
/// user-defined from→to pairs.
pub fn apply_word_replacements(text: &str, replacements: &[WordReplacement]) -> String {
    if replacements.is_empty() {
        return text.to_string();
    }

    let mut result = text.to_string();
    for replacement in replacements {
        let escaped = regex::escape(&replacement.from);
        if let Ok(pattern) = Regex::new(&format!(r"(?i)\b{}\b", escaped)) {
            result = pattern.replace_all(&result, replacement.to.as_str()).to_string();
        }
    }

    // Clean up any double spaces from replacements
    MULTI_SPACE_PATTERN.replace_all(&result, " ").trim().to_string()
}

/// Filters transcription output by removing filler words and stutter artifacts.
///
/// This function cleans up raw transcription text by:
/// 1. Removing filler words based on the app language (or custom list)
/// 2. Collapsing repeated 1-2 letter stutters (e.g., "wh wh wh" -> "wh")
/// 3. Cleaning up excess whitespace
///
/// # Arguments
/// * `text` - The raw transcription text to filter
/// * `lang` - The app language code (e.g., "en", "pt-BR") used to select filler words
/// * `custom_filler_words` - Optional user-provided filler word list. `Some(vec)` overrides
///   language defaults; `Some(empty vec)` disables filtering; `None` uses language defaults.
///
/// # Returns
/// The filtered text with filler words and stutters removed
pub fn filter_transcription_output(
    text: &str,
    lang: &str,
    custom_filler_words: &Option<Vec<String>>,
) -> String {
    let mut filtered = text.to_string();

    // Build filler patterns from custom list or language defaults
    let patterns: Vec<Regex> = match custom_filler_words {
        Some(words) => words
            .iter()
            .filter_map(|word| Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(word))).ok())
            .collect(),
        None => get_filler_words_for_language(lang)
            .iter()
            .map(|word| Regex::new(&format!(r"(?i)\b{}\b[,.]?", regex::escape(word))).unwrap())
            .collect(),
    };

    // Remove filler words
    for pattern in &patterns {
        filtered = pattern.replace_all(&filtered, "").to_string();
    }

    // Collapse repeated 1-2 letter words (stutter artifacts like "wh wh wh wh")
    filtered = collapse_stutters(&filtered);

    // Clean up multiple spaces to single space
    filtered = MULTI_SPACE_PATTERN.replace_all(&filtered, " ").to_string();

    // Trim leading/trailing whitespace
    filtered.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_custom_words_exact_match() {
        let text = "hello world";
        let custom_words = vec!["Hello".to_string(), "World".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_apply_custom_words_fuzzy_match() {
        let text = "helo wrold";
        let custom_words = vec!["hello".to_string(), "world".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_preserve_case_pattern() {
        assert_eq!(preserve_case_pattern("HELLO", "world"), "WORLD");
        assert_eq!(preserve_case_pattern("Hello", "world"), "World");
        assert_eq!(preserve_case_pattern("hello", "WORLD"), "WORLD");
    }

    #[test]
    fn test_extract_punctuation() {
        assert_eq!(extract_punctuation("hello"), ("", ""));
        assert_eq!(extract_punctuation("!hello?"), ("!", "?"));
        assert_eq!(extract_punctuation("...hello..."), ("...", "..."));
    }

    #[test]
    fn test_empty_custom_words() {
        let text = "hello world";
        let custom_words = vec![];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_filter_filler_words() {
        let text = "So uhm I was thinking uh about this";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "So I was thinking about this");
    }

    #[test]
    fn test_filter_filler_words_case_insensitive() {
        let text = "UHM this is UH a test";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "this is a test");
    }

    #[test]
    fn test_filter_filler_words_with_punctuation() {
        let text = "Well, uhm, I think, uh. that's right";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Well, I think, that's right");
    }

    #[test]
    fn test_filter_cleans_whitespace() {
        let text = "Hello    world   test";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Hello world test");
    }

    #[test]
    fn test_filter_trims() {
        let text = "  Hello world  ";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_filter_combined() {
        let text = "  Uhm, so I was, uh, thinking about this  ";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "so I was, thinking about this");
    }

    #[test]
    fn test_filter_preserves_valid_text() {
        let text = "This is a completely normal sentence.";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "This is a completely normal sentence.");
    }

    #[test]
    fn test_filter_stutter_collapse() {
        let text = "w wh wh wh wh wh wh wh wh wh why";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "w wh why");
    }

    #[test]
    fn test_filter_stutter_short_words() {
        let text = "I I I I think so so so so";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "I think so");
    }

    #[test]
    fn test_filter_stutter_mixed_case() {
        let text = "No NO no NO no";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "No");
    }

    #[test]
    fn test_filter_stutter_preserves_two_repetitions() {
        let text = "no no is fine";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "no no is fine");
    }

    #[test]
    fn test_filter_english_removes_um() {
        let text = "um I think um this is good";
        let result = filter_transcription_output(text, "en", &None);
        assert_eq!(result, "I think this is good");
    }

    #[test]
    fn test_filter_portuguese_preserves_um() {
        // "um" means "a/an" in Portuguese
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt", &None);
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_spanish_preserves_ha() {
        // "ha" means "has" in Spanish
        let text = "ha sido un buen día";
        let result = filter_transcription_output(text, "es", &None);
        assert_eq!(result, "ha sido un buen día");
    }

    #[test]
    fn test_filter_language_code_with_region() {
        // "pt-BR" should normalize to "pt"
        let text = "um gato bonito";
        let result = filter_transcription_output(text, "pt-BR", &None);
        assert_eq!(result, "um gato bonito");
    }

    #[test]
    fn test_filter_custom_filler_words_override() {
        let custom = Some(vec!["okay".to_string(), "right".to_string()]);
        let text = "okay so I think right this works";
        let result = filter_transcription_output(text, "en", &custom);
        assert_eq!(result, "so I think this works");
    }

    #[test]
    fn test_filter_custom_filler_words_empty_disables() {
        let custom = Some(vec![]);
        let text = "So uhm I was thinking uh about this";
        let result = filter_transcription_output(text, "en", &custom);
        // No filler words removed since custom list is empty
        assert_eq!(result, "So uhm I was thinking uh about this");
    }

    #[test]
    fn test_filter_unknown_language_uses_fallback() {
        let text = "uh I think uhm this works";
        let result = filter_transcription_output(text, "xx", &None);
        assert_eq!(result, "I think this works");
    }

    #[test]
    fn test_filter_fallback_does_not_remove_um() {
        // Fallback (unknown language) should not remove "um" since it's a real word in some languages
        let text = "um I think this works";
        let result = filter_transcription_output(text, "xx", &None);
        assert_eq!(result, "um I think this works");
    }

    #[test]
    fn test_apply_custom_words_ngram_two_words() {
        let text = "il cui nome è Charge B, che permette";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("ChargeBee,"));
        assert!(!result.contains("Charge B"));
    }

    #[test]
    fn test_apply_custom_words_ngram_three_words() {
        let text = "use Chat G P T for this";
        let custom_words = vec!["ChatGPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("ChatGPT"));
    }

    #[test]
    fn test_apply_custom_words_prefers_longer_ngram() {
        let text = "Open AI GPT model";
        let custom_words = vec!["OpenAI".to_string(), "GPT".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert_eq!(result, "OpenAI GPT model");
    }

    #[test]
    fn test_apply_custom_words_ngram_preserves_case() {
        let text = "CHARGE B is great";
        let custom_words = vec!["ChargeBee".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("CHARGEBEE"));
    }

    #[test]
    fn test_apply_custom_words_ngram_with_spaces_in_custom() {
        // Custom word with space should also match against split words
        let text = "using Mac Book Pro";
        let custom_words = vec!["MacBook Pro".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        assert!(result.contains("MacBook"));
    }

    #[test]
    fn test_apply_custom_words_trailing_number_not_doubled() {
        // Verify that trailing non-alpha chars (like numbers) aren't double-counted
        // between build_ngram stripping them and extract_punctuation capturing them
        let text = "use GPT4 for this";
        let custom_words = vec!["GPT-4".to_string()];
        let result = apply_custom_words(text, &custom_words, 0.5);
        // Should NOT produce "GPT-44" (double-counting the trailing 4)
        assert!(
            !result.contains("GPT-44"),
            "got double-counted result: {}",
            result
        );
    }

    #[test]
    fn test_apply_word_replacements_basic() {
        use crate::settings::WordReplacement;
        let text = "I'm gonna do it";
        let replacements = vec![WordReplacement {
            from: "gonna".to_string(),
            to: "going to".to_string(),
        }];
        let result = apply_word_replacements(text, &replacements);
        assert_eq!(result, "I'm going to do it");
    }

    #[test]
    fn test_apply_word_replacements_case_insensitive() {
        use crate::settings::WordReplacement;
        let text = "I'm Gonna do it";
        let replacements = vec![WordReplacement {
            from: "gonna".to_string(),
            to: "going to".to_string(),
        }];
        let result = apply_word_replacements(text, &replacements);
        assert_eq!(result, "I'm going to do it");
    }

    #[test]
    fn test_apply_word_replacements_multiple() {
        use crate::settings::WordReplacement;
        let text = "I wanna go but I gotta stay";
        let replacements = vec![
            WordReplacement { from: "wanna".to_string(), to: "want to".to_string() },
            WordReplacement { from: "gotta".to_string(), to: "got to".to_string() },
        ];
        let result = apply_word_replacements(text, &replacements);
        assert_eq!(result, "I want to go but I got to stay");
    }

    #[test]
    fn test_apply_word_replacements_empty() {
        let text = "hello world";
        let replacements: Vec<crate::settings::WordReplacement> = vec![];
        let result = apply_word_replacements(text, &replacements);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_apply_word_replacements_no_partial_match() {
        use crate::settings::WordReplacement;
        let text = "the megaphone is here";
        let replacements = vec![WordReplacement {
            from: "mega".to_string(),
            to: "super".to_string(),
        }];
        let result = apply_word_replacements(text, &replacements);
        assert_eq!(result, "the megaphone is here");
    }

    #[test]
    fn test_apply_word_replacements_multi_word_from() {
        use crate::settings::WordReplacement;
        let text = "I need to sort of figure this out";
        let replacements = vec![WordReplacement {
            from: "sort of".to_string(),
            to: "somewhat".to_string(),
        }];
        let result = apply_word_replacements(text, &replacements);
        assert_eq!(result, "I need to somewhat figure this out");
    }

    #[test]
    fn test_apply_word_replacements_special_chars_no_panic() {
        use crate::settings::WordReplacement;
        let text = "I use c++ daily";
        let replacements = vec![WordReplacement {
            from: "c++".to_string(),
            to: "C plus plus".to_string(),
        }];
        let result = apply_word_replacements(text, &replacements);
        // \b word boundaries don't match around non-word chars like +,
        // so this won't replace — but regex::escape ensures no panic
        assert_eq!(result, "I use c++ daily");
    }

    #[test]
    fn test_collapse_marker_basic() {
        let result = collapse_self_corrections(
            "Send it to marketing, I mean send it to sales",
            &None,
        );
        assert_eq!(result, "send it to sales");
    }

    #[test]
    fn test_collapse_marker_or_rather() {
        let result = collapse_self_corrections(
            "The meeting is Tuesday, or rather it's Wednesday",
            &None,
        );
        assert_eq!(result, "it's Wednesday");
    }

    #[test]
    fn test_collapse_marker_scratch_that() {
        let result = collapse_self_corrections(
            "Open the file, scratch that, close the file",
            &None,
        );
        assert_eq!(result, "close the file");
    }

    #[test]
    fn test_collapse_marker_case_insensitive() {
        let result = collapse_self_corrections(
            "Go left, I Mean go right",
            &None,
        );
        assert_eq!(result, "go right");
    }

    #[test]
    fn test_collapse_marker_at_start_is_noop() {
        let result = collapse_self_corrections(
            "I mean this is the right way",
            &None,
        );
        assert_eq!(result, "I mean this is the right way");
    }

    #[test]
    fn test_collapse_marker_multiple() {
        let result = collapse_self_corrections(
            "Go up, no wait go down, I mean go left",
            &None,
        );
        assert_eq!(result, "go left");
    }

    #[test]
    fn test_collapse_marker_no_punctuation_fallback() {
        let result = collapse_self_corrections(
            "Send it to marketing I mean send it to sales",
            &None,
        );
        assert_eq!(result, "send it to sales");
    }

    #[test]
    fn test_collapse_marker_custom_markers() {
        let custom = Some(vec!["oops".to_string()]);
        let result = collapse_self_corrections(
            "Take the left turn, oops take the right turn",
            &custom,
        );
        assert_eq!(result, "take the right turn");
    }

    #[test]
    fn test_collapse_marker_empty_custom_disables_markers() {
        let custom = Some(vec![]);
        let result = collapse_self_corrections(
            "Go left, I mean go right",
            &custom,
        );
        // Marker detection disabled, but false-start detection still runs.
        // These clauses don't share an opener so false-start won't fire either.
        assert_eq!(result, "Go left, I mean go right");
    }

    #[test]
    fn test_collapse_no_correction_passthrough() {
        let result = collapse_self_corrections(
            "This is a perfectly normal sentence.",
            &None,
        );
        assert_eq!(result, "This is a perfectly normal sentence.");
    }

    #[test]
    fn test_collapse_empty_passthrough() {
        let result = collapse_self_corrections("", &None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_collapse_marker_with_em_dash() {
        let result = collapse_self_corrections(
            "Send it to marketing\u{2014}I mean send it to sales",
            &None,
        );
        assert_eq!(result, "send it to sales");
    }

    #[test]
    fn test_false_start_basic() {
        let result = collapse_self_corrections(
            "We should probably... We need to ship by Friday",
            &None,
        );
        assert_eq!(result, "We need to ship by Friday");
    }

    #[test]
    fn test_false_start_longer_first_clause_preserved() {
        // First clause is longer — not a false start, it's emphasis or elaboration
        let result = collapse_self_corrections(
            "We absolutely need to ship this week, we need to ship",
            &None,
        );
        assert_eq!(result, "We absolutely need to ship this week, we need to ship");
    }

    #[test]
    fn test_false_start_different_opener_preserved() {
        let result = collapse_self_corrections(
            "The budget is tight, we need more resources",
            &None,
        );
        assert_eq!(result, "The budget is tight, we need more resources");
    }

    #[test]
    fn test_false_start_with_the() {
        let result = collapse_self_corrections(
            "The report, the quarterly report is ready",
            &None,
        );
        assert_eq!(result, "the quarterly report is ready");
    }

    #[test]
    fn test_mixed_marker_and_false_start() {
        // "I mean" removes "We should", then false-start collapses the repeated opener
        let result = collapse_self_corrections(
            "We should, I mean we need to, we need to ship by Friday",
            &None,
        );
        assert_eq!(result, "we need to ship by Friday");
    }

    #[test]
    fn test_collapse_marker_multi_sentence_fallback() {
        // Fallback marker detection with no punctuation removes everything before the marker.
        // For v1 this is acceptable since transcription fragments are typically single sentences.
        let result = collapse_self_corrections(
            "The project is on track I mean we need more time",
            &None,
        );
        assert_eq!(result, "we need more time");
    }
}
