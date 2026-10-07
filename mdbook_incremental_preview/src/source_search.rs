use super::*;
use pulldown_cmark::{Event, Options, Parser};

pub fn encode_url_path(path: &Path) -> String {
    path.to_string_lossy()
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Mark unchanged top-level Markdown blocks with their original source line.
/// Transformed blocks have no source position rather than an invented one.
pub fn mark_source(content: &mut String, source: &str, path: &Path) {
    let mut depth = 0;
    let mut markers = Vec::new();
    let mut html_end = None;
    let mut source_depth = 0;
    let mut source_blocks = Vec::new();
    for (event, range) in Parser::new_ext(source, Options::all()).into_offset_iter() {
        let start = matches!(event, Event::Start(_));
        match event {
            Event::Start(_) | Event::Rule => {
                if source_depth == 0 {
                    source_blocks.push(range);
                }
                if start {
                    source_depth += 1;
                }
            }
            Event::End(_) => source_depth -= 1,
            _ => {}
        }
    }
    let path = path
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    for (event, range) in Parser::new_ext(content, Options::all()).into_offset_iter() {
        let start = matches!(event, Event::Start(_));
        match event {
            Event::Start(_) | Event::Rule => {
                if depth == 0 {
                    let block = &content[range.clone()];
                    let offset = if content == source {
                        Some(range.start)
                    } else {
                        let mut matches = source_blocks
                            .iter()
                            .filter(|range| &source[(*range).clone()] == block);
                        let first = matches.next().map(|range| range.start);
                        if matches.next().is_none() && content.matches(block).count() == 1 {
                            first
                        } else {
                            None
                        }
                    };
                    let line = offset
                        .map(|offset| {
                            source[..offset]
                                .bytes()
                                .filter(|b| *b == b'\n')
                                .count()
                                .to_string()
                        })
                        .unwrap_or_default();
                    let line_start = content[..range.start]
                        .rfind('\n')
                        .map_or(0, |offset| offset + 1);
                    let start = if content[line_start..range.start].trim().is_empty() {
                        line_start
                    } else {
                        range.start
                    };
                    markers.push((start, format!("<div class=\"mdbook-source-marker\" data-source-path=\"{path}\" data-source-line=\"{line}\" hidden></div>\n\n")));
                }
                if start {
                    depth += 1;
                }
            }
            Event::End(_) => depth -= 1,
            Event::Html(_) if depth == 0 => {
                if html_end != Some(range.start) {
                    markers.push((range.start, format!("<div class=\"mdbook-source-marker\" data-source-path=\"{path}\" data-source-line=\"\" hidden></div>\n\n")));
                }
                html_end = Some(range.end);
            }
            _ => {}
        }
    }
    for (offset, marker) in markers.into_iter().rev() {
        content.insert_str(offset, &marker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_markers_preserve_rendered_blocks() {
        let source =
            "# Heading\n\nparagraph **bold**\n\n- item\n- next\n\n```rust\nlet x = 1;\n```\n\n    indented code\n\n> quotation\n\n| first | second |\n| --- | --- |\n| a | b |\n\n---\n\n<div>html</div>\n\nend\n";
        let mut marked = source.to_owned();
        mark_source(&mut marked, source, Path::new("chapter.md"));
        let render = |markdown: &str| {
            let mut html = String::new();
            pulldown_cmark::html::push_html(&mut html, Parser::new_ext(markdown, Options::all()));
            html
        };
        let html = render(&marked);
        let without_markers =
            regex::Regex::new(r#"<div class="mdbook-source-marker"[^>]*></div>\n"#)
                .expect("Valid marker regex")
                .replace_all(&html, "");
        assert_eq!(without_markers, render(source));
        assert!(marked.contains("data-source-line=\"4\""));
    }

    #[test]
    fn transformed_and_ambiguous_blocks_are_unmapped() {
        let source = "same\n\nsame\n\noriginal\n\nlast\n";
        let mut content = "same\n\nchanged\n\nlast\n".to_owned();
        mark_source(&mut content, source, Path::new("chapter.md"));
        assert_eq!(content.matches("data-source-line=\"\"").count(), 2);
        assert!(content.contains("data-source-line=\"6\""));
    }
}
