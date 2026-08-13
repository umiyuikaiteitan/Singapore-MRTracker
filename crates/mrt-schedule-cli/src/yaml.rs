//! A small YAML reader for the configuration file.
//!
//! The parser reads the subset of YAML that a configuration file
//! needs and turns it into a [`serde_json::Value`], which serde then
//! deserializes into [`mrt_publication::PublicationConfig`]. Going
//! through serde keeps one definition of the configuration schema.
//!
//! # The supported subset
//!
//! - Block mappings with two-space (or any consistent) indentation.
//! - Block sequences with `- `.
//! - Scalars: plain, single-quoted, and double-quoted.
//! - `true`, `false`, `null`, `~`, integers, and floats.
//! - Empty flow collections, `{}` and `[]`.
//! - Comments from an unquoted `#` to the end of the line.
//! - A leading `---` document marker.
//!
//! # What it does not read
//!
//! Anchors, aliases, tags, multi-line scalars, non-empty flow
//! collections, and multiple documents. The parser reports a line
//! number and says what it found, so a file that needs more than this
//! subset fails loudly instead of silently losing a value.

use std::collections::BTreeMap;

use serde_json::Value;

/// A parse failure, with the line that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    /// The one-based line number.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for YamlError {}

/// One significant line of the file.
#[derive(Debug, Clone)]
struct Line {
    number: usize,
    indent: usize,
    content: String,
}

/// Parse a YAML document into a JSON value.
pub fn parse(source: &str) -> Result<Value, YamlError> {
    let lines = significant_lines(source)?;
    if lines.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let mut cursor = 0usize;
    let value = parse_block(&lines, &mut cursor, lines[0].indent)?;
    if cursor < lines.len() {
        return Err(YamlError {
            line: lines[cursor].number,
            message: format!(
                "unexpected indentation; \"{}\" does not belong to the block above it",
                lines[cursor].content
            ),
        });
    }
    Ok(value)
}

/// Drop blank lines, comments, and the document marker.
fn significant_lines(source: &str) -> Result<Vec<Line>, YamlError> {
    let mut out = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        // YAML forbids a tab in indentation, and a tab that reaches
        // the parser silently shifts a whole block.
        if raw
            .chars()
            .take_while(|c| c.is_whitespace())
            .any(|c| c == '\t')
        {
            return Err(YamlError {
                line: number,
                message: "YAML forbids a tab in indentation; use spaces".to_string(),
            });
        }
        let indent = raw.len() - raw.trim_start_matches(' ').len();
        let content = strip_comment(&raw[indent..]);
        let content = content.trim_end();
        if content.is_empty() || content == "---" {
            continue;
        }
        out.push(Line {
            number,
            indent,
            content: content.to_string(),
        });
    }
    Ok(out)
}

/// Remove a trailing comment that is not inside a quoted scalar.
fn strip_comment(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    for (index, &byte) in bytes.iter().enumerate() {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'#' && (index == 0 || bytes[index - 1] == b' ') => {
                return &text[..index];
            }
            None => {}
        }
    }
    text
}

/// Parse the block that starts at `cursor` and is indented by
/// `indent`.
fn parse_block(lines: &[Line], cursor: &mut usize, indent: usize) -> Result<Value, YamlError> {
    if lines[*cursor].content.starts_with("- ") || lines[*cursor].content == "-" {
        parse_sequence(lines, cursor, indent)
    } else {
        parse_mapping(lines, cursor, indent)
    }
}

fn parse_mapping(lines: &[Line], cursor: &mut usize, indent: usize) -> Result<Value, YamlError> {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    while *cursor < lines.len() {
        let line = &lines[*cursor];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(YamlError {
                line: line.number,
                message: "unexpected indentation inside a mapping".to_string(),
            });
        }
        let Some((key, rest)) = split_key(&line.content) else {
            return Err(YamlError {
                line: line.number,
                message: format!("expected \"key: value\", found \"{}\"", line.content),
            });
        };
        let number = line.number;
        *cursor += 1;
        let value = if rest.is_empty() {
            match child_indent(lines, *cursor, indent) {
                Some(child) => parse_block(lines, cursor, child)?,
                // A key with nothing under it is an empty mapping,
                // which is how an "unset" section reads.
                None => Value::Object(serde_json::Map::new()),
            }
        } else {
            scalar(rest, number)?
        };
        if map.insert(key.clone(), value).is_some() {
            return Err(YamlError {
                line: number,
                message: format!("the key \"{key}\" appears twice in the same mapping"),
            });
        }
    }
    Ok(Value::Object(map.into_iter().collect()))
}

fn parse_sequence(lines: &[Line], cursor: &mut usize, indent: usize) -> Result<Value, YamlError> {
    let mut items = Vec::new();
    while *cursor < lines.len() {
        let line = &lines[*cursor];
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return Err(YamlError {
                line: line.number,
                message: "unexpected indentation inside a sequence".to_string(),
            });
        }
        let Some(rest) = line
            .content
            .strip_prefix("- ")
            .or_else(|| (line.content == "-").then_some(""))
        else {
            break;
        };
        let number = line.number;
        let rest = rest.trim();
        if rest.is_empty() {
            *cursor += 1;
            match child_indent(lines, *cursor, indent) {
                Some(child) => items.push(parse_block(lines, cursor, child)?),
                None => items.push(Value::Null),
            }
        } else if let Some((key, tail)) = split_key(rest) {
            // "- key: value" starts a mapping whose first key sits on
            // the dash line. Its indentation is the dash plus two.
            let inner_indent = indent + 2;
            let mut map: BTreeMap<String, Value> = BTreeMap::new();
            let first = if tail.is_empty() {
                *cursor += 1;
                match child_indent(lines, *cursor, inner_indent - 1) {
                    Some(child) => parse_block(lines, cursor, child)?,
                    None => Value::Object(serde_json::Map::new()),
                }
            } else {
                *cursor += 1;
                scalar(tail, number)?
            };
            map.insert(key, first);
            if let Value::Object(rest_map) = parse_mapping(lines, cursor, inner_indent)? {
                for (key, value) in rest_map {
                    map.insert(key, value);
                }
            }
            items.push(Value::Object(map.into_iter().collect()));
        } else {
            items.push(scalar(rest, number)?);
            *cursor += 1;
        }
    }
    Ok(Value::Array(items))
}

/// Get the indentation of the block that belongs to the key above it.
fn child_indent(lines: &[Line], cursor: usize, parent: usize) -> Option<usize> {
    lines
        .get(cursor)
        .filter(|line| line.indent > parent)
        .map(|line| line.indent)
}

/// Split `key: value`, honouring quotes in the key.
fn split_key(content: &str) -> Option<(String, &str)> {
    let bytes = content.as_bytes();
    let mut quote: Option<u8> = None;
    for (index, &byte) in bytes.iter().enumerate() {
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b':' && (index + 1 == bytes.len() || bytes[index + 1] == b' ') => {
                let key = unquote(content[..index].trim());
                if key.is_empty() {
                    return None;
                }
                return Some((key, content[index + 1..].trim()));
            }
            None => {}
        }
    }
    None
}

fn unquote(text: &str) -> String {
    let bytes = text.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

/// Turn a scalar into a JSON value.
fn scalar(text: &str, line: usize) -> Result<Value, YamlError> {
    let text = text.trim();
    if text.starts_with('"') || text.starts_with('\'') {
        if text.len() < 2 || !text.ends_with(text.as_bytes()[0] as char) {
            return Err(YamlError {
                line,
                message: format!("the quoted value {text} is not closed"),
            });
        }
        // A double-quoted scalar carries the usual escapes.
        let inner = &text[1..text.len() - 1];
        return Ok(Value::String(if text.starts_with('"') {
            unescape(inner)
        } else {
            inner.replace("''", "'")
        }));
    }
    Ok(match text {
        "" | "null" | "~" => Value::Null,
        "true" | "True" | "yes" | "on" => Value::Bool(true),
        "false" | "False" | "no" | "off" => Value::Bool(false),
        "{}" => Value::Object(serde_json::Map::new()),
        "[]" => Value::Array(Vec::new()),
        other => {
            if let Ok(number) = other.parse::<i64>() {
                Value::Number(number.into())
            } else if let Some(number) = other
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
            {
                Value::Number(number)
            } else {
                Value::String(other.to_string())
            }
        }
    })
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_flat_mapping_parses() {
        let value = parse("version: 1\nprofile: singapore-lta\n").unwrap();
        assert_eq!(value, json!({"version": 1, "profile": "singapore-lta"}));
    }

    #[test]
    fn nested_mappings_follow_the_indentation() {
        let value = parse(
            "timetable:\n  layout: responsive\n  columns: 2\n  title:\n    en: A\n    ja: B\n",
        )
        .unwrap();
        assert_eq!(
            value,
            json!({"timetable": {"layout": "responsive", "columns": 2,
                                 "title": {"en": "A", "ja": "B"}}})
        );
    }

    #[test]
    fn sequences_of_scalars_parse() {
        let value = parse("font_stack:\n  - Noto Sans\n  - Arial\n  - sans-serif\n").unwrap();
        assert_eq!(
            value,
            json!({"font_stack": ["Noto Sans", "Arial", "sans-serif"]})
        );
    }

    #[test]
    fn sequences_of_mappings_parse() {
        let value =
            parse("corridors:\n  - id: main\n    line: EX\n    axis:\n      - EX1\n      - EX2\n")
                .unwrap();
        assert_eq!(
            value,
            json!({"corridors": [{"id": "main", "line": "EX", "axis": ["EX1", "EX2"]}]})
        );
    }

    #[test]
    fn nested_sequences_of_mappings_parse() {
        let source = "corridors:\n\
                      \x20 - id: main\n\
                      \x20   axis:\n\
                      \x20     - A\n\
                      \x20   branches:\n\
                      \x20     - junction: A\n\
                      \x20       axis:\n\
                      \x20         - B\n";
        let value = parse(source).unwrap();
        assert_eq!(
            value,
            json!({"corridors": [{
                "id": "main",
                "axis": ["A"],
                "branches": [{"junction": "A", "axis": ["B"]}],
            }]})
        );
    }

    #[test]
    fn scalars_keep_their_types() {
        let value = parse(
            "a: 1\nb: 1.5\nc: true\nd: false\ne: null\nf: ~\ng: \"07\"\nh: 07:00:00\ni: {}\nj: []\n",
        )
        .unwrap();
        assert_eq!(value["a"], json!(1));
        assert_eq!(value["b"], json!(1.5));
        assert_eq!(value["c"], json!(true));
        assert_eq!(value["d"], json!(false));
        assert_eq!(value["e"], json!(null));
        assert_eq!(value["f"], json!(null));
        // A quoted value stays a string, which is what a time needs.
        assert_eq!(value["g"], json!("07"));
        assert_eq!(value["h"], json!("07:00:00"));
        assert_eq!(value["i"], json!({}));
        assert_eq!(value["j"], json!([]));
    }

    #[test]
    fn comments_and_blank_lines_disappear() {
        let value = parse(
            "# a comment\n\nversion: 1  # trailing\n\n# another\nprofile: \"x # not a comment\"\n",
        )
        .unwrap();
        assert_eq!(value, json!({"version": 1, "profile": "x # not a comment"}));
    }

    #[test]
    fn a_document_marker_is_ignored() {
        assert_eq!(parse("---\nversion: 1\n").unwrap(), json!({"version": 1}));
    }

    #[test]
    fn an_empty_document_is_an_empty_mapping() {
        assert_eq!(parse("").unwrap(), json!({}));
        assert_eq!(parse("# only a comment\n").unwrap(), json!({}));
    }

    #[test]
    fn a_key_without_a_value_is_an_empty_mapping() {
        assert_eq!(
            parse("labels:\nversion: 1\n").unwrap(),
            json!({"labels": {}, "version": 1})
        );
    }

    #[test]
    fn escapes_in_double_quotes_resolve() {
        let value = parse("a: \"line\\nbreak\"\nb: 'it''s'\n").unwrap();
        assert_eq!(value["a"], json!("line\nbreak"));
        assert_eq!(value["b"], json!("it's"));
    }

    #[test]
    fn a_duplicate_key_is_an_error() {
        let error = parse("a: 1\na: 2\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("twice"));
    }

    #[test]
    fn a_line_that_is_not_a_mapping_entry_is_an_error() {
        let error = parse("version: 1\njust some text\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("key: value"));
    }

    #[test]
    fn an_unclosed_quote_is_an_error() {
        let error = parse("a: \"unfinished\n").unwrap_err();
        assert!(error.message.contains("not closed"));
    }

    #[test]
    fn a_tab_in_the_indentation_is_an_error() {
        let error = parse("a:\n\t- 1\n").unwrap_err();
        assert!(error.message.contains("tab"));
    }

    #[test]
    fn the_example_configuration_of_the_documentation_parses() {
        let source = include_str!("../../../config/singapore.yaml");
        let value = parse(source).unwrap();
        assert_eq!(value["version"], json!(1));
        assert_eq!(value["timezone"], json!("Asia/Singapore"));
        assert_eq!(value["day_start"], json!("04:00:00"));
    }
}
