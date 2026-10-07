//! Bounded, offline release-note formatting. No HTML or remote image loading.
use crate::*;
use iced::widget::column;

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Heading,
    Paragraph,
    Bullet(String),
    Code,
    Rule,
}

#[derive(Debug, PartialEq, Eq)]
struct Block {
    kind: Kind,
    text: String,
    links: Vec<(String, String)>,
}

pub(crate) fn safe_url(value: &str) -> Option<String> {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

fn inline(value: &str) -> (String, Vec<(String, String)>) {
    let mut text = String::new();
    let mut links = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('[') {
        let Some(close) = rest[start + 1..].find("](").map(|n| n + start + 1) else {
            break;
        };
        let Some(end) = rest[close + 2..].find(')').map(|n| n + close + 2) else {
            break;
        };
        let image = start > 0 && rest.as_bytes()[start - 1] == b'!';
        text.push_str(&rest[..start - usize::from(image)]);
        let title = &rest[start + 1..close];
        text.push_str(title);
        if !image
            && links.len() < 8
            && let Some(url) = safe_url(&rest[close + 2..end])
        {
            links.push((title.to_string(), url));
        }
        rest = &rest[end + 1..];
    }
    text.push_str(rest);
    // Inline emphasis is presented as plain readable text; code fences retain
    // their exact contents. Link destinations are only opened on an explicit click.
    (
        text.replace("**", "").replace("__", "").replace('`', ""),
        links,
    )
}

fn parse(notes: &str) -> (Vec<Block>, bool) {
    const LIMIT: usize = 160;
    let clipped: String = notes.chars().take(12000).collect();
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut code = None::<String>;
    let mut truncated = notes.chars().count() > 12000;
    let flush = |text: &mut String, blocks: &mut Vec<Block>| {
        if !text.is_empty() {
            let (text, links) = inline(&std::mem::take(text));
            blocks.push(Block {
                kind: Kind::Paragraph,
                text,
                links,
            });
        }
    };
    for line in clipped.lines() {
        if blocks.len() >= LIMIT {
            truncated = true;
            break;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut paragraph, &mut blocks);
            if let Some(text) = code.take() {
                blocks.push(Block {
                    kind: Kind::Code,
                    text,
                    links: vec![],
                });
            } else {
                code = Some(String::new());
            }
            continue;
        }
        if let Some(code) = code.as_mut() {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }
        let heading = trimmed.trim_start_matches('#');
        let heading = ((1..=6).contains(&(trimmed.len() - heading.len()))
            && heading.starts_with(' '))
        .then(|| heading.trim());
        let bullet = ["- ", "* ", "+ "]
            .into_iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix).map(|text| ("•".into(), text)))
            .or_else(|| {
                let (number, text) = trimmed.split_once(". ")?;
                (!number.is_empty()
                    && number.len() <= 3
                    && number.bytes().all(|n| n.is_ascii_digit()))
                .then(|| (format!("{number}."), text))
            });
        let kind_and_text = if let Some(text) = heading {
            Some((Kind::Heading, text))
        } else if let Some((prefix, text)) = bullet {
            Some((Kind::Bullet(prefix), text))
        } else if ["---", "***", "___"].contains(&trimmed) {
            Some((Kind::Rule, ""))
        } else {
            None
        };
        if let Some((kind, text)) = kind_and_text {
            flush(&mut paragraph, &mut blocks);
            let (text, links) = inline(text);
            blocks.push(Block { kind, text, links });
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(trimmed.strip_prefix("> ").unwrap_or(trimmed));
        }
    }
    flush(&mut paragraph, &mut blocks);
    if let Some(text) = code {
        blocks.push(Block {
            kind: Kind::Code,
            text,
            links: vec![],
        });
    }
    (blocks, truncated)
}

impl App {
    pub(crate) fn release_link<'a>(&self, title: &str, url: &str) -> Element<'a, Message> {
        let Some(url) = safe_url(url) else {
            return Column::new().into();
        };
        button(label(
            format!("{title} ↗"),
            14.0,
            Weight::Semibold,
            self.edition.accent(),
        ))
        .padding([4, 0])
        .on_press_maybe((!self.exporting).then_some(Message::OpenReleaseLink(url)))
        .style(button::text)
        .into()
    }

    pub(crate) fn release_notes<'a>(&self, notes: &str, url: &str) -> Element<'a, Message> {
        let (blocks, truncated) = parse(notes);
        let mut body = Column::new().spacing(12).width(Length::Fill);
        for block in blocks {
            let standalone_link = block.kind == Kind::Paragraph
                && block.links.len() == 1
                && block.text == block.links[0].0;
            let content: Element<'_, Message> = match block.kind {
                Kind::Heading => label(block.text, 18.0, Weight::Semibold, INK).into(),
                Kind::Paragraph => self.paragraph(block.text),
                Kind::Bullet(prefix) => row![
                    label(prefix, 15.0, Weight::Semibold, self.edition.accent()).width(26),
                    self.paragraph(block.text)
                ]
                .spacing(6)
                .into(),
                Kind::Code => container(
                    iced::widget::text(block.text)
                        .size(13)
                        .font(Font::MONOSPACE),
                )
                .padding(12)
                .width(Length::Fill)
                .style(|_| container::Style::default().background(BG).color(INK))
                .into(),
                Kind::Rule => separator(),
            };
            if !standalone_link {
                body = body.push(content);
            }
            for (title, url) in block.links {
                body = body.push(self.release_link(&title, &url));
            }
        }
        if truncated {
            body = body.push(
                self.paragraph(
                    self.tr(
                        "Weitere Hinweise stehen in der vollständigen Release-Seite.",
                        "More notes are available on the full release page.",
                    )
                    .to_string(),
                ),
            );
        }
        column![
            separator(),
            row![
                label(
                    self.tr("Was ist neu?", "What's new?"),
                    16.0,
                    Weight::Semibold,
                    INK
                ),
                Space::new().width(Length::Fill),
                self.release_link(self.tr("Release öffnen", "Open release"), url)
            ]
            .spacing(12)
            .align_y(alignment::Vertical::Center),
            scrollable(container(body).padding(Padding {
                right: 20.0,
                ..Padding::ZERO
            }))
            .height(300)
            .width(Length::Fill)
        ]
        .spacing(14)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_keep_structure_and_only_explicit_https_links() {
        let (blocks, truncated) = parse(
            "## Improvements\n\nA **readable** paragraph\ncontinued here.\n\n- Fixed `Start`\n1. [Details](https://example.com/changes)\n\n```sh\n# literal code\ncommand --flag\n```\n![tracking](https://example.com/pixel.png) [bad](javascript:alert(1))",
        );
        assert!(!truncated);
        assert_eq!(blocks[0].kind, Kind::Heading);
        assert_eq!(blocks[1].text, "A readable paragraph continued here.");
        assert_eq!(blocks[2].kind, Kind::Bullet("•".into()));
        assert_eq!(
            blocks[3].links,
            [("Details".into(), "https://example.com/changes".into())]
        );
        assert_eq!(blocks[4].kind, Kind::Code);
        assert_eq!(blocks[4].text, "# literal code\ncommand --flag");
        assert!(blocks[5].links.is_empty());
        for url in [
            "file:///tmp/file",
            "javascript:alert(1)",
            "http://example.com",
            "https://user:pass@example.com",
            "https://example.com/\n",
        ] {
            assert!(safe_url(url).is_none(), "{url}");
        }
    }

    #[test]
    fn excessively_long_notes_are_bounded() {
        let (blocks, truncated) = parse(&"- change\n".repeat(10000));
        assert!(truncated);
        assert!(blocks.len() <= 161);
    }
}
