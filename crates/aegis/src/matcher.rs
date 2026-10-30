/// one open tab to rank against the command bar input
pub struct TabCandidate<'a> {
    pub title: &'a str,
    pub display_host: &'a str,
    pub last_active_ms: u64,
}

/// scores a case-insensitive subsequence match
pub fn score(query: &str, text: &str) -> Option<u32> {
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some(0);
    }
    let mut matched = 0;
    let mut points = 0_u32;
    let mut previous_match = false;
    let mut previous = None;
    let mut length = 0_u32;
    for (index, original) in text.chars().enumerate() {
        length = length.saturating_add(1);
        let word_start = previous.is_none_or(|ch| matches!(ch, ' ' | '.' | '/' | '-' | '_'));
        let mut here = false;
        for lower in original.to_lowercase() {
            if matched < query.len() && query[matched] == lower {
                points = points.saturating_add(10);
                if index == 0 && matched == 0 {
                    points = points.saturating_add(30);
                }
                if word_start {
                    points = points.saturating_add(18);
                }
                if previous_match || here {
                    points = points.saturating_add(12);
                }
                matched += 1;
                here = true;
            }
        }
        previous_match = here;
        previous = Some(original);
    }
    if matched != query.len() {
        return None;
    }
    Some(points.saturating_add(100_u32.saturating_sub(length.min(100))))
}

/// ranks up to six tabs by match quality and recency
pub fn rank_tabs(query: &str, tabs: &[TabCandidate<'_>]) -> Vec<(usize, u32)> {
    let mut ranked = Vec::new();
    for (index, tab) in tabs.iter().enumerate() {
        let title = score(query, tab.title);
        let host = score(query, tab.display_host).map(|value| value.saturating_add(4));
        if let Some(points) = title.into_iter().chain(host).max() {
            ranked.push((index, points));
        }
    }
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| tabs[b.0].last_active_ms.cmp(&tabs[a.0].last_active_ms))
    });
    ranked.truncate(6);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_shapes() {
        assert!(score("arc", "arc") > score("arc", "archive"));
        assert!(score("arc", "archive") > score("arc", "search arc"));
        assert!(score("arc", "x/arc") > score("arc", "xparc"));
        assert!(score("arc", "a random choice").is_some());
        assert_eq!(score("xyz", "archive"), None);
        assert_eq!(score("", "archive"), Some(0));
    }

    #[test]
    fn case_and_unicode() {
        assert_eq!(score("Ã¼", "Ãœber"), score("Ãœ", "Ã¼ber"));
        assert!(score("a", "A").is_some());
        assert!(score("i\u{307}", "İ").is_some());
    }

    #[test]
    fn ties_use_recency() {
        let tabs = [
            TabCandidate {
                title: "same",
                display_host: "one.test",
                last_active_ms: 1,
            },
            TabCandidate {
                title: "same",
                display_host: "two.test",
                last_active_ms: 2,
            },
        ];
        assert_eq!(rank_tabs("same", &tabs)[0].0, 1);
    }

    #[test]
    fn long_inputs() {
        let input = "a".repeat(10_000);
        assert!(score(&input, &input).is_some());
        assert_eq!(score(&input, "short"), None);
    }
}
