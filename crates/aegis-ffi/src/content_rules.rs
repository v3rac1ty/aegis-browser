#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn build_json(substrings: &[String]) -> String {
    let mut json = String::from("[");
    for substring in substrings {
        if substring.is_empty() {
            continue;
        }
        if json.len() > 1 {
            json.push(',');
        }
        json.push_str("{\"trigger\":{\"url-filter\":\"");
        for character in substring.chars() {
            if character == '\\' {
                json.push_str("\\\\\\\\");
                continue;
            }
            if matches!(
                character,
                '.' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|'
            ) {
                json.push_str("\\\\");
            }
            match character {
                '"' => json.push_str("\\\""),
                '\n' => json.push_str("\\n"),
                '\r' => json.push_str("\\r"),
                '\t' => json.push_str("\\t"),
                c if c < ' ' => json.push_str(&format!("\\u{:04x}", c as u32)),
                c => json.push(c),
            }
        }
        json.push_str("\"},\"action\":{\"type\":\"block\"}}");
    }
    json.push(']');
    json
}

#[cfg(test)]
mod tests {
    use super::build_json;

    #[test]
    fn escapes_regex_and_json() {
        let rules = vec!["a.b?c*d\\e\"f".to_owned()];
        assert_eq!(
            build_json(&rules),
            r#"[{"trigger":{"url-filter":"a\\.b\\?c\\*d\\\\e\"f"},"action":{"type":"block"}}]"#
        );
    }

    #[test]
    fn empty_list() {
        assert_eq!(build_json(&[]), "[]");
    }

    #[test]
    fn skips_empty_substrings() {
        let rules = vec![String::new(), "ads".to_owned()];
        assert_eq!(
            build_json(&rules),
            r#"[{"trigger":{"url-filter":"ads"},"action":{"type":"block"}}]"#
        );
    }
}
