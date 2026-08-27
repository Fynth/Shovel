pub fn trim_completion_for_cursor(sql: &str, cursor: usize, completion: &str) -> String {
    let mut completion = completion
        .trim_matches(|ch| matches!(ch, '\r' | '\n'))
        .to_string();
    if completion.is_empty() {
        return completion;
    }

    let token_range = current_token_range(sql, cursor);
    let typed_token = &sql[token_range.start..cursor];
    if !typed_token.is_empty() && completion.starts_with(typed_token) {
        completion = completion[typed_token.len()..].to_string();
    }

    let suffix = &sql[cursor..];
    let prefix_overlap = common_prefix_byte_len(suffix, &completion);
    if prefix_overlap > 0 {
        completion = completion[prefix_overlap..].to_string();
    }

    let suffix_overlap = suffix_prefix_overlap_byte_len(suffix, &completion);
    if suffix_overlap > 0 {
        completion.truncate(completion.len() - suffix_overlap);
    }

    completion
}

fn current_token_range(sql: &str, cursor: usize) -> std::ops::Range<usize> {
    let mut index = cursor.min(sql.len());
    while index > 0 && !sql.is_char_boundary(index) {
        index -= 1;
    }

    let mut range_start = index;
    for (offset, ch) in sql[..index].char_indices().rev() {
        if is_token_boundary(ch) {
            break;
        }
        range_start = offset;
    }

    let mut range_end = index;
    for (offset, ch) in sql[index..].char_indices() {
        if is_token_boundary(ch) {
            break;
        }
        range_end = index + offset + ch.len_utf8();
    }

    range_start..range_end
}

fn is_token_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | ';'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '+'
                | '-'
                | '*'
                | '/'
                | '='
                | '<'
                | '>'
                | ':'
        )
}

fn common_prefix_byte_len(left: &str, right: &str) -> usize {
    let mut byte_len = 0;
    for (left_ch, right_ch) in left.chars().zip(right.chars()) {
        if left_ch != right_ch {
            break;
        }
        byte_len += right_ch.len_utf8();
    }
    byte_len
}

fn suffix_prefix_overlap_byte_len(suffix: &str, completion: &str) -> usize {
    let mut best_overlap = 0;
    let mut suffix_prefix_len = 0;
    for ch in suffix.chars() {
        suffix_prefix_len += ch.len_utf8();
        if completion.ends_with(&suffix[..suffix_prefix_len]) {
            best_overlap = suffix_prefix_len;
        }
    }
    best_overlap
}

#[cfg(test)]
mod tests {
    use super::trim_completion_for_cursor;

    #[test]
    fn trim_completion_removes_repeated_token_and_suffix_overlap() {
        let sql = "sel from users";
        let cursor = "sel".len();

        assert_eq!(
            trim_completion_for_cursor(sql, cursor, "select from users"),
            "ect"
        );
    }

    #[test]
    fn trim_completion_at_mid_document_caret() {
        let sql = "select  from users";
        let cursor = "select ".len();
        assert_eq!(
            trim_completion_for_cursor(sql, cursor, "id, name from users"),
            "id, name"
        );
    }
}
