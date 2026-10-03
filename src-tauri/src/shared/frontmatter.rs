//! Minimal YAML-style frontmatter reader for Markdown prompt, command and skill files.

/// Reads a leading `---` block and returns its `key: value` pairs (keys
/// lowercased, surrounding quotes removed) with the byte offset where the body
/// starts. Folded (`>`) and literal (`|`) block values are joined into one line.
/// Returns `None` when the content has no closed frontmatter block.
pub(crate) fn parse_frontmatter_fields(content: &str) -> Option<(Vec<(String, String)>, usize)> {
    let mut segments = content.split_inclusive('\n').peekable();
    let first_segment = segments.next()?;
    if first_segment.trim_end_matches(['\r', '\n']).trim() != "---" {
        return None;
    }

    let mut fields: Vec<(String, String)> = Vec::new();
    let mut consumed = first_segment.len();
    while let Some(segment) = segments.next() {
        consumed += segment.len();
        let trimmed = segment.trim_end_matches(['\r', '\n']).trim();
        if trimmed == "---" {
            return Some((fields, consumed));
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let mut value = value.trim().to_string();
        if matches!(value.as_str(), ">" | ">-" | "|" | "|-") {
            let mut lines = Vec::new();
            while let Some(next) = segments.peek() {
                let line = next.trim_end_matches(['\r', '\n']);
                if line.trim() == "---" || !(line.starts_with(' ') || line.starts_with('\t') || line.trim().is_empty()) {
                    break;
                }
                consumed += next.len();
                if !line.trim().is_empty() {
                    lines.push(line.trim().to_string());
                }
                segments.next();
            }
            value = lines.join(" ");
        } else if value.len() >= 2 {
            let bytes = value.as_bytes();
            let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
            if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
                value = value[1..value.len() - 1].to_string();
            }
        }
        fields.push((key.trim().to_ascii_lowercase(), value));
    }
    None
}

/// Returns the value of `key` from parsed frontmatter fields.
pub(crate) fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fields_and_body_offset() {
        let content = "---\nname: graphify\ndescription: \"Build a graph\"\n# comment\nargument-hint: '<path>'\n---\nBody\n";
        let (fields, offset) = parse_frontmatter_fields(content).expect("frontmatter");
        assert_eq!(field(&fields, "name"), Some("graphify"));
        assert_eq!(field(&fields, "description"), Some("Build a graph"));
        assert_eq!(field(&fields, "argument-hint"), Some("<path>"));
        assert_eq!(&content[offset..], "Body\n");
    }

    #[test]
    fn joins_block_values() {
        let content = "---\ndescription: >\n  First line\n  second line\nname: x\n---\n";
        let (fields, _) = parse_frontmatter_fields(content).expect("frontmatter");
        assert_eq!(field(&fields, "description"), Some("First line second line"));
        assert_eq!(field(&fields, "name"), Some("x"));
    }

    #[test]
    fn rejects_missing_or_unclosed_frontmatter() {
        assert!(parse_frontmatter_fields("# Title\n").is_none());
        assert!(parse_frontmatter_fields("---\nname: x\n").is_none());
        assert!(parse_frontmatter_fields("").is_none());
    }
}
