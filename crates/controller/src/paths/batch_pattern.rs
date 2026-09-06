use glob::Pattern;

const MAX_ALTERNATIVES: usize = 64;

/// Compile alternatives within one path component without shell expansion.
pub(super) fn compile(segment: &str) -> Result<Vec<Pattern>, &'static str> {
    let mut expanded = vec![String::new()];
    let mut literal = String::new();
    let mut alternatives = Vec::new();
    let mut in_group = false;
    let mut in_class = false;
    for character in segment.chars() {
        match character {
            '[' if !in_class => {
                in_class = true;
                literal.push(character);
            }
            ']' if in_class => {
                in_class = false;
                literal.push(character);
            }
            '{' if !in_class => {
                if in_group {
                    return Err("Nested brace alternatives are not supported.");
                }
                append_literal(&mut expanded, &literal);
                literal.clear();
                in_group = true;
            }
            ',' if in_group && !in_class => {
                push_alternative(&mut alternatives, &mut literal)?;
            }
            '}' if !in_class => {
                if !in_group {
                    return Err("Unmatched closing brace in input pattern.");
                }
                push_alternative(&mut alternatives, &mut literal)?;
                if alternatives.len() < 2 {
                    return Err("Brace groups require at least two comma-separated alternatives.");
                }
                if expanded.len() * alternatives.len() > MAX_ALTERNATIVES {
                    return Err("A path component exceeds 64 wildcard alternatives.");
                }
                expanded = expanded
                    .iter()
                    .flat_map(|prefix| {
                        alternatives
                            .iter()
                            .map(move |suffix| format!("{prefix}{suffix}"))
                    })
                    .collect();
                alternatives.clear();
                in_group = false;
            }
            _ => literal.push(character),
        }
    }
    if in_group {
        return Err("Unclosed brace group in input pattern.");
    }
    append_literal(&mut expanded, &literal);
    expanded
        .into_iter()
        .map(|value| {
            if value == "." || value == ".." {
                return Err("Parent traversal and ambiguous paths are not allowed.");
            }
            if value == "**" && segment != "**" {
                return Err("Recursive ** must be a standalone path component outside braces.");
            }
            Pattern::new(&value).map_err(|_| "Invalid wildcard syntax in input pattern.")
        })
        .collect()
}

fn append_literal(expanded: &mut [String], literal: &str) {
    for value in expanded {
        value.push_str(literal);
    }
}

fn push_alternative(
    alternatives: &mut Vec<String>,
    literal: &mut String,
) -> Result<(), &'static str> {
    if literal.is_empty() {
        return Err("Brace alternatives must not be empty.");
    }
    if alternatives.len() == MAX_ALTERNATIVES {
        return Err("A path component exceeds 64 wildcard alternatives.");
    }
    alternatives.push(std::mem::take(literal));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compile;

    #[test]
    fn alternatives_preserve_glob_character_classes_and_literal_braces(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (pattern, matches, misses) in [
            ("E?.{mkv,mp4}", "E1.mp4", "E12.mp4"),
            ("E{01,02}.{mkv,mp4}", "E02.mkv", "E03.mkv"),
            ("file.{[am]kv,mp4}", "file.mkv", "file.ass"),
            ("[{]mkv,mp4[}]", "{mkv,mp4}", "mkv"),
            ("file[{},].mkv", "file,.mkv", "fileA.mkv"),
        ] {
            let patterns = compile(pattern)?;
            assert!(
                patterns.iter().any(|pattern| pattern.matches(matches)),
                "{pattern}"
            );
            assert!(
                !patterns.iter().any(|pattern| pattern.matches(misses)),
                "{pattern}"
            );
        }
        Ok(())
    }

    #[test]
    fn combined_groups_obey_expansion_limit_and_reject_traversal() {
        assert_eq!(
            compile(&"{a,b}".repeat(6)).map(|patterns| patterns.len()),
            Ok(64)
        );
        assert!(compile(&"{a,b}".repeat(7)).is_err());
        for pattern in ["{..,safe}", "{.,safe}", "{,mkv}", "{mkv,,mp4}", "{1..12}"] {
            assert!(compile(pattern).is_err(), "{pattern}");
        }
    }
}
