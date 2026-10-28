//! Input minimizer for failing inputs.

/// Minimizes a failing string input while `fails` predicate returns `true`.
pub fn minimize<F>(input: &str, mut fails: F) -> String
where
    F: FnMut(&str) -> bool,
{
    if !fails(input) {
        return input.to_string();
    }

    let mut current = input.to_string();

    // 1. Chunk reduction: try removing chunks of decreasing sizes
    let mut chunk_size = current.len() / 2;
    while chunk_size > 0 {
        let mut i = 0;
        while i + chunk_size <= current.len() {
            if current.is_char_boundary(i) && current.is_char_boundary(i + chunk_size) {
                let mut candidate = String::with_capacity(current.len() - chunk_size);
                candidate.push_str(&current[..i]);
                candidate.push_str(&current[i + chunk_size..]);

                if fails(&candidate) {
                    current = candidate;
                    continue;
                }
            }
            i += 1;
        }
        chunk_size /= 2;
    }

    // 2. Line reduction: try removing individual lines
    let mut line_idx = 0;
    while line_idx < current.lines().count() {
        let lines: Vec<&str> = current.lines().collect();
        if lines.len() <= 1 {
            break;
        }
        let mut candidate_lines = lines;
        candidate_lines.remove(line_idx);
        let candidate = candidate_lines.join("\n");
        if fails(&candidate) {
            current = candidate;
        } else {
            line_idx += 1;
        }
    }

    // 3. Character reduction: try removing individual characters
    let mut char_idx = 0;
    while char_idx < current.chars().count() {
        let mut candidate = String::with_capacity(current.len());
        for (idx, ch) in current.chars().enumerate() {
            if idx != char_idx {
                candidate.push(ch);
            }
        }
        if fails(&candidate) {
            current = candidate;
        } else {
            char_idx += 1;
        }
    }

    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimizer() {
        let input = "abcdef_CRASH_ghijkl";
        let minimized = minimize(input, |s| s.contains("CRASH"));
        assert_eq!(minimized, "CRASH");
    }
}
