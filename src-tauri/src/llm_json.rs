/// Local LLMs (llama2 in particular) don't reliably follow "respond ONLY
/// with JSON" instructions — they often wrap the JSON object in explanatory
/// prose, or precede/follow it with extra text. Parsing the raw response
/// directly makes that prose a parse failure, which callers then treat as
/// "the model found nothing" even when it clearly did. This pulls out just
/// the outermost `{...}` block so parsing succeeds regardless of the wrapper
/// text around it.
pub fn extract_json_object(text: &str) -> &str {
    match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if end > start => &text[start..=end],
        _ => text,
    }
}

/// Last-resort recovery for a numeric field when the model emits something
/// JSON-*shaped* but not valid JSON (seen in practice: `"matched_id": id 3`
/// instead of `"matched_id": 3`). Scans past `field_name` for the first
/// digit run, but bails out if "null" appears first — that's a real "no
/// match", not a formatting slip.
pub fn recover_number_field(text: &str, field_name: &str) -> Option<u64> {
    let after = text.find(field_name)?;
    let tail = &text[after + field_name.len()..];
    let stop = tail.find(['\n', ',', '}']).unwrap_or(tail.len());
    let window = &tail[..stop];

    if let Some(null_pos) = window.to_lowercase().find("null") {
        if let Some(digit_pos) = window.find(|c: char| c.is_ascii_digit()) {
            if null_pos < digit_pos {
                return None;
            }
        } else {
            return None;
        }
    }

    let digits: String = window.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_wrapped_in_prose() {
        let text = "Thank you! Here's the answer:\n{ \"matched_id\": 3, \"reason\": \"mentions gm\" }\nHope that helps!";
        assert_eq!(extract_json_object(text), "{ \"matched_id\": 3, \"reason\": \"mentions gm\" }");
    }

    #[test]
    fn recovers_bareword_id() {
        let text = "{ \"matched_id\": id 3, \"reason\": \"lowest AVAX\" }";
        assert_eq!(recover_number_field(text, "matched_id"), Some(3));
    }

    #[test]
    fn recovers_plain_null_as_no_match() {
        let text = "{ \"matched_id\": null, \"reason\": \"nothing fits\" }";
        assert_eq!(recover_number_field(text, "matched_id"), None);
    }

    #[test]
    fn does_not_recover_a_number_that_only_appears_after_null() {
        let text = "{ \"matched_id\": null, \"reason\": \"listing 3 doesn't match\" }";
        assert_eq!(recover_number_field(text, "matched_id"), None);
    }
}
