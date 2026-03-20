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
| Ordinals (irregular) | "first"→1, "second"→2, "third"→3, "fifth"→5, "eighth"→8, "ninth"→9, "twelfth"→12 | value + irregular suffix |
| Ordinals (regular) | Any cardinal stem + "th" ("fourth"→4, "sixth"→6, "seventh"→7, "tenth"→10, "eleventh"→11, etc.) | value + "th" |
| Ordinals (tens) | "twentieth"→20, "thirtieth"→30, etc. ("-ieth" suffix) | value + "th" |

### Parsing

The parser scans text word-by-word, accumulating number tokens into a buffer. When a non-number word is encountered, the buffer is flushed and converted.

**Token handling:** Before matching each word against lookup tables, strip trailing punctuation (commas, periods, etc.) and reattach it after conversion. This follows the same pattern as `extract_punctuation` in the existing custom words code.

**Composition rules:**
1. Units/teens combine with tens: "twenty" + "three" → 23
2. "hundred" multiplies the current group: "two" + "hundred" → 200, then adds: + "fifty" + "three" → 253
3. "thousand"/"million"/"billion" multiplies the accumulated total, then a new group starts: "two" + "thousand" + "three" + "hundred" → 2300
4. "and" is transparent only when the tokens on both sides are number words: "one hundred and fifty" → 150. If the next token after "and" is not a number word, "and" is flushed as literal text.

**Decimal handling:** "point" splits the number buffer into two independent accumulators. The left accumulator is flushed as the integer part, "." is emitted, then a new accumulator starts for the fractional part. The fractional part concatenates individual digits rather than composing them: "two point five" → "2.5", "three point one four" → "3.14" (each word after "point" is converted to its single digit and appended).

**Ordinal handling:** When the last word in a number buffer is an ordinal form (e.g., "third", "twenty-third", "fiftieth"), the parser recognizes it as its cardinal value and marks the buffer as ordinal. On flush, the appropriate suffix ("st", "nd", "rd", "th") is appended to the digit output.

### Threshold Rule

Single-word numbers from "zero" through "ten" inclusive (values 0-10) are preserved as words unless followed by a pattern trigger (currency, percentage). Multi-word numbers and single-word numbers ≥ 11 ("eleven" and above) are always converted.

Ordinals follow the same threshold: single-word ordinals whose cardinal value is ≤ 10 ("first" through "tenth") are preserved as words unless a pattern trigger is present.

**Rationale:** Low single-digit words frequently appear as natural language ("I have one question", "the two of us") rather than quantities. Multi-word numbers ("twenty three") are almost always meant as actual numbers.

**Exception:** Pattern triggers override the threshold. "five dollars" → "$5" and "three percent" → "3%" even though 5 and 3 are ≤ 10.

### Pattern Detection

Pattern triggers are checked by lookahead on the word immediately following the number buffer, before flushing. If a trigger word is found, it modifies the output format and is consumed (not emitted as text).

| Pattern | Trigger word(s) | Output | Example |
|---------|-----------------|--------|---------|
| Currency | "dollar(s)", "buck(s)" after number | "$" prefix, trigger word consumed | "five dollars" → "$5", "twenty bucks" → "$20" |
| Percentage | "percent" after number | "%" suffix, trigger word consumed | "three percent" → "3%" |
| Decimal | "point" between number groups | "." separator, see Decimal handling above | "two point five" → "2.5" |
| Ordinal | ordinal form of last number word | digit + suffix ("st"/"nd"/"rd"/"th") | "twenty third" → "23rd" |

### Example Walkthrough

Input: `"I need twenty three items at five dollars and three percent discount"`

| Token(s) | Rule | Output |
|----------|------|--------|
| "I need" | non-number | "I need" |
| "twenty three" | multi-word cardinal, above threshold | "23" |
| "items at" | non-number, flushes buffer | "items at" |
| "five" | number, enters buffer | (buffered) |
| "dollars" | pattern trigger: currency, overrides threshold | "$5" |
| "and" | next token "three" is a number, but "and" is only transparent inside a number group; here it starts a new context | "and" |
| "three" | number, enters buffer | (buffered) |
| "percent" | pattern trigger: percentage, overrides threshold | "3%" |
| "discount" | non-number | "discount" |

Result: `"I need 23 items at $5 and 3% discount"`

## Settings

### New Field in `AppSettings`

```rust
#[serde(default = "default_number_normalization_enabled")]
pub number_normalization_enabled: bool,  // default: true
```

On by default, no UI for v1. No custom word lists — the number vocabulary is fixed English.

Also add `number_normalization_enabled: true` to `get_default_settings()`.

## File Changes

| File | Change |
|------|--------|
| `src-tauri/src/audio_toolkit/text.rs` | Add lookup tables, `normalize_numbers()`, helper functions, and unit tests |
| `src-tauri/src/audio_toolkit/mod.rs` | Re-export `normalize_numbers` |
| `src-tauri/src/settings.rs` | Add `number_normalization_enabled` field, default function, and `get_default_settings()` entry |
| `src-tauri/src/managers/transcription.rs` | Import + call site after self-correction (replace `let final_result = self_corrected_result` binding) |

## Testing

15 unit tests:

1. Multi-word cardinals: "twenty three" → "23"
2. Large cardinals: "two thousand three hundred forty five" → "2345"
3. "one hundred and fifty" → "150" ("and" ignored between number words)
4. Threshold preserved (≤ 10): "I have three dogs" → unchanged
5. Threshold preserved (zero): "I have zero issues" → unchanged
6. Threshold converted (≥ 11): "I have twelve dogs" → "I have 12 dogs"
7. Currency overrides threshold: "five dollars" → "$5"
8. Percentage overrides threshold: "three percent" → "3%"
9. Currency above threshold: "twenty bucks" → "$20"
10. Decimals: "two point five" → "2.5"
11. Ordinals above threshold: "twenty third" → "23rd"
12. Ordinals under threshold preserved: "first place" → unchanged
13. Mixed text: "I need twenty three items" → "I need 23 items"
14. Punctuation preserved: "twenty three," → "23,"
15. No numbers / empty: passthrough
