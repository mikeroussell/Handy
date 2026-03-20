# Number Normalization Design Spec

**Date:** 2026-03-20
**Feature:** Inverse text normalization for spelled-out numbers in transcription output
**Status:** Approved

## Goal

Convert spelled-out numbers in transcribed text to their digit form ("twenty three" → "23") as a post-processing step in Handy's transcription pipeline, with zero latency impact.

## Scope

**Included (v1):**
- Cardinal numbers: "twenty three" → "23", "one hundred fifty" → "150"
- Ordinal numbers: "twenty third" → "23rd", "first" → "1st"
- Currency: "five dollars" → "$5", "ten bucks" → "$10"
- Percentages: "three percent" → "3%"
- Decimals: "two point five" → "2.5"

**Excluded (future):**
- Dates: "March twentieth" → "March 20th"
- Times: "two thirty" → "2:30"
- Phone numbers: "five five five one two three four" → "555-1234"
- Non-English languages

## Architecture

### Pipeline Position

Runs as the final text processing step, after self-correction:

```
Audio → Whisper/Parakeet → custom words → word replacements → filler removal → self-correction → number normalization → final result
```

Rationale: self-correction may discard clauses containing number words, so normalize last to avoid wasted work.

### Implementation Approach

Regex-based mapping with static lookup tables. No new dependencies. Pure string manipulation on already-transcribed text — microsecond processing time.

### New Function

`pub fn normalize_numbers(text: &str) -> String` in `src-tauri/src/audio_toolkit/text.rs`

## Algorithm

### Lookup Tables

| Table | Keys | Values |
|-------|------|--------|
| Units | "zero" through "nineteen" | 0-19 |
| Tens | "twenty" through "ninety" | 20-90 |
| Magnitudes | "hundred", "thousand", "million", "billion" | 100, 1000, 1000000, 1000000000 |
| Ordinal suffixes | "first"→"1st", "second"→"2nd", "third"→"3rd", "fifth"→"5th", "eighth"→"8th", "ninth"→"9th", "twelfth"→"12th", plus "-ieth"→"-ieth" pattern | digit + suffix |

### Parsing

The parser scans text word-by-word, accumulating number tokens into a buffer. When a non-number word is encountered, the buffer is flushed and converted.

**Composition rules:**
1. Units/teens combine with tens: "twenty" + "three" → 23
2. "hundred" multiplies the current group: "two" + "hundred" → 200, then adds: + "fifty" + "three" → 253
3. "thousand"/"million"/"billion" multiplies the accumulated total, then a new group starts: "two" + "thousand" + "three" + "hundred" → 2300
4. "and" between number words is ignored: "one hundred and fifty" → 150

### Threshold Rule

Single-word numbers ≤ 10 ("one" through "ten") are preserved as words unless followed by a pattern trigger (currency, percentage). Multi-word numbers and single-word numbers ≥ 11 are always converted.

**Rationale:** Low single-digit words frequently appear as natural language ("I have one question", "the two of us") rather than quantities. Multi-word numbers ("twenty three") are almost always meant as actual numbers.

**Exception:** Pattern triggers override the threshold. "five dollars" → "$5" and "three percent" → "3%" even though 5 and 3 are ≤ 10.

### Pattern Detection

Applied after cardinal parsing, before flushing the buffer:

| Pattern | Trigger word(s) | Output | Example |
|---------|-----------------|--------|---------|
| Currency | "dollar(s)", "buck(s)" after number | "$" prefix | "five dollars" → "$5" |
| Percentage | "percent" after number | "%" suffix | "three percent" → "3%" |
| Decimal | "point" between number groups | "." separator | "two point five" → "2.5" |
| Ordinal | ordinal form of number word | digit + suffix | "twenty third" → "23rd" |

### Example Walkthrough

Input: `"I need twenty three items at five dollars and three percent discount"`

| Token(s) | Rule | Output |
|----------|------|--------|
| "I need" | non-number | "I need" |
| "twenty three" | multi-word cardinal, above threshold | "23" |
| "items at" | non-number | "items at" |
| "five dollars" | ≤ 10 but currency pattern trigger | "$5" |
| "and" | non-number | "and" |
| "three percent" | ≤ 10 but percentage pattern trigger | "3%" |
| "discount" | non-number | "discount" |

Result: `"I need 23 items at $5 and 3% discount"`

## Settings

### New Field in `AppSettings`

```rust
#[serde(default = "default_number_normalization_enabled")]
pub number_normalization_enabled: bool,  // default: true
```

On by default, no UI for v1. No custom word lists — the number vocabulary is fixed English.

## File Changes

| File | Change |
|------|--------|
| `src-tauri/src/audio_toolkit/text.rs` | Add lookup tables, `normalize_numbers()`, helper functions, and unit tests |
| `src-tauri/src/audio_toolkit/mod.rs` | Re-export `normalize_numbers` |
| `src-tauri/src/settings.rs` | Add `number_normalization_enabled` field and default function |
| `src-tauri/src/managers/transcription.rs` | Import + call site after self-correction |

## Testing

13 unit tests:

1. Multi-word cardinals: "twenty three" → "23"
2. Large cardinals: "two thousand three hundred forty five" → "2345"
3. "one hundred and fifty" → "150" (and ignored)
4. Threshold preserved: "I have three dogs" → unchanged
5. Threshold converted: "I have twelve dogs" → "I have 12 dogs"
6. Currency overrides threshold: "five dollars" → "$5"
7. Percentage overrides threshold: "three percent" → "3%"
8. Currency: "twenty bucks" → "$20"
9. Decimals: "two point five" → "2.5"
10. Ordinals: "twenty third" → "23rd"
11. Ordinals under threshold preserved: "first place" → unchanged
12. Mixed text: "I need twenty three items" → "I need 23 items"
13. No numbers / empty: passthrough
