
use regex::{Captures, Regex};
use std::sync::LazyLock;

const MAX_IMAGES: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Text(String),
    Image { url: String, alt: String, link: Option<String> },
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid markdown pattern")
}

static COMMENT: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<!--.*?-->"));
static IMG_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<img\b[^>]*>"));
static ATTR: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)([a-z-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#));
static ANCHOR: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<a\b([^>]*)>(.*?)</a>"));
static AUTOLINK: LazyLock<Regex> = LazyLock::new(|| re(r"<(https?://[^>\s]+)>"));
static BREAK: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<br\s*/?>"));
static HEADING_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<(?:h[1-6]|summary)\b[^>]*>(.*?)</(?:h[1-6]|summary)>"));
static BOLD_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<(?:b|strong)\b[^>]*>(.*?)</(?:b|strong)>"));
static ITALIC_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<(?:i|em)\b[^>]*>(.*?)</(?:i|em)>"));
static STRIKE_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<(?:s|del|strike)\b[^>]*>(.*?)</(?:s|del|strike)>"));
static CODE_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?is)<code\b[^>]*>(.*?)</code>"));
static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)<li\b[^>]*>"));
static BLOCK_END: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)</(?:p|div|center|details|table|tr|li|ul|ol|blockquote)>"));
static ANY_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"</?[a-zA-Z][^>]*>"));
static HEADING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{0,3}#{1,6}\s+(.*?)(?:\s+#+)?\s*$"));
static TASK: LazyLock<Regex> = LazyLock::new(|| re(r"^(\s*(?:[-*+]|\d+[.)])\s+)\[[ xX]\]\s+"));
static REFERENCE_DEF: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{0,3}\[[^\]]+\]:\s+\S+.*$"));
static LOOSE_STRONG: LazyLock<Regex> = LazyLock::new(|| re(r"\*\*[ \t]*([^*\n]*?[^*\s])[ \t]*\*\*"));
static BLANK_RUN: LazyLock<Regex> = LazyLock::new(|| re(r"\n{3,}"));
static IMAGE: LazyLock<Regex> = LazyLock::new(|| {
    re(r#"(?x)
        (?P<open>\[\s*)?
        !\[(?P<alt>[^\]]*)\]
        \(\s*<?(?P<src>[^)\s>]+)>?(?:\s+"[^"]*")?\s*\)
        (?:\s*\]\(\s*<?(?P<href>[^)\s>]+)>?(?:\s+"[^"]*")?\s*\))?
    "#)
});

pub fn parse(source: &str) -> Vec<Block> {
    let markdown = normalize_blocks(&strip_html(source));
    let markdown = LOOSE_STRONG.replace_all(&markdown, "**$1**");
    split_images(&markdown)
}

fn strip_html(source: &str) -> String {
    let text = COMMENT.replace_all(source, "");
    let text = IMG_TAG.replace_all(&text, |caps: &Captures| {
        let tag = &caps[0];
        match attribute(tag, "src") {
            Some(src) => format!("\n\n![{}]({src})\n\n", attribute(tag, "alt").unwrap_or_default().replace(['[', ']'], "")),
            None => String::new(),
        }
    });
    let text = ANCHOR.replace_all(&text, |caps: &Captures| {
        let inner = caps[2].trim();
        match attribute(&caps[1], "href") {
            Some(href) if !inner.is_empty() && is_web(&href) => format!("[{inner}]({href})"),
            _ => inner.to_string(),
        }
    });
    let text = AUTOLINK.replace_all(&text, "[$1]($1)");
    let text = BREAK.replace_all(&text, "\n\n");
    let text = HEADING_TAG.replace_all(&text, "\n\n**$1**\n\n");
    let text = BOLD_TAG.replace_all(&text, "**$1**");
    let text = ITALIC_TAG.replace_all(&text, "*$1*");
    let text = STRIKE_TAG.replace_all(&text, "~~$1~~");
    let text = CODE_TAG.replace_all(&text, "`$1`");
    let text = LIST_ITEM.replace_all(&text, "\n- ");
    let text = BLOCK_END.replace_all(&text, "\n\n");
    let text = ANY_TAG.replace_all(&text, "");
    text.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    ATTR.captures_iter(tag).find(|c| c[1].eq_ignore_ascii_case(name)).and_then(|c| c.get(2).or_else(|| c.get(3)).map(|m| m.as_str().trim().to_string())).filter(|v| !v.is_empty())
}

fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn normalize_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            if !trimmed.is_empty() {
                out.push_str(&format!("`{}`\n\n", trimmed.replace('`', "'")));
            }
            continue;
        }
        if REFERENCE_DEF.is_match(line) || is_rule(trimmed) || is_table_divider(trimmed) {
            out.push('\n');
        } else if let Some(caps) = HEADING.captures(line) {
            out.push_str(&format!("\n**{}**\n\n", caps[1].replace("**", "")));
        } else if trimmed.starts_with('|') {
            let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
            out.push_str(&format!("{}\n\n", cells.join("  ·  ")));
        } else {
            let line = if trimmed.starts_with('>') { trimmed.trim_start_matches(['>', ' ']) } else { line };
            out.push_str(&TASK.replace(line, "$1"));
            out.push('\n');
        }
    }
    BLANK_RUN.replace_all(&out, "\n\n").into_owned()
}

fn is_rule(trimmed: &str) -> bool {
    let mut marks = trimmed.chars().filter(|c| !c.is_whitespace());
    match marks.next() {
        Some(first @ ('-' | '*' | '_')) => trimmed.chars().filter(|c| !c.is_whitespace()).count() >= 3 && marks.all(|c| c == first),
        _ => false,
    }
}

fn is_table_divider(trimmed: &str) -> bool {
    trimmed.contains('-') && trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) && trimmed.contains('|')
}

fn split_images(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut images = 0;
    let mut cursor = 0;
    for caps in IMAGE.captures_iter(markdown) {
        let whole = caps.get(0).expect("whole match");
        push_text(&mut blocks, &markdown[cursor..whole.start()]);
        cursor = whole.end();
        let Some(url) = web_url(&caps["src"]) else { continue };
        if images == MAX_IMAGES {
            continue;
        }
        images += 1;
        let link = caps.name("href").map(|m| m.as_str()).filter(|_| caps.name("open").is_some()).and_then(web_url);
        blocks.push(Block::Image { url, alt: caps["alt"].trim().to_string(), link });
    }
    push_text(&mut blocks, &markdown[cursor..]);
    blocks
}

fn push_text(blocks: &mut Vec<Block>, text: &str) {
    let text = BLANK_RUN.replace_all(text.trim(), "\n\n");
    if !text.is_empty() && text.chars().any(char::is_alphanumeric) {
        blocks.push(Block::Text(text.into_owned()));
    }
}

fn web_url(url: &str) -> Option<String> {
    match url {
        u if is_web(u) => Some(u.to_string()),
        u if u.starts_with("//") => Some(format!("https:{u}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Block {
        Block::Text(s.into())
    }

    #[test]
    fn lifts_badges_with_links() {
        let blocks = parse("[![Buy me a coffee](https://img.example/coffee.png)](https://ko-fi.com/x)\n\nHello");
        assert_eq!(
            blocks,
            vec![Block::Image { url: "https://img.example/coffee.png".into(), alt: "Buy me a coffee".into(), link: Some("https://ko-fi.com/x".into()) }, text("Hello")]
        );
    }

    #[test]
    fn converts_centered_html_images() {
        let blocks = parse(r#"<center><img src="https://a.test/banner.png" alt="Banner"></center>"#);
        assert_eq!(blocks, vec![Block::Image { url: "https://a.test/banner.png".into(), alt: "Banner".into(), link: None }]);
    }

    #[test]
    fn linked_html_images_keep_their_link() {
        let blocks = parse(r#"<a href="https://x.test"><img src='https://a.test/b.png'></a>"#);
        assert_eq!(blocks, vec![Block::Image { url: "https://a.test/b.png".into(), alt: String::new(), link: Some("https://x.test".into()) }]);
    }

    #[test]
    fn repairs_bold_with_padding() {
        assert_eq!(parse("** This mod is the result **of work"), vec![text("**This mod is the result**of work")]);
    }

    #[test]
    fn rewrites_unsupported_blocks() {
        let blocks = parse("# Title\n\n> quoted\n\n---\n\n- [x] done\n\n| a | b |\n|---|---|\n| 1 | 2 |");
        let Block::Text(body) = &blocks[0] else { panic!("expected text") };
        assert!(body.starts_with("**Title**"));
        assert!(body.contains("quoted") && !body.contains('>'));
        assert!(!body.contains("---") && !body.contains("[x]"));
        assert!(body.contains("1  ·  2"));
        assert!(slint::StyledText::from_markdown(body).is_ok());
    }

    #[test]
    fn details_become_headed_sections_and_stay_valid() {
        let blocks = parse("<details><summary>More</summary>\n\nInside <b>bold</b> and <a href=\"https://x.test\">link</a>\n\n</details>");
        let Block::Text(body) = &blocks[0] else { panic!("expected text") };
        assert!(body.contains("**More**") && body.contains("**bold**") && body.contains("[link](https://x.test)"));
        assert!(slint::StyledText::from_markdown(body).is_ok());
    }

    #[test]
    fn drops_relative_and_unsafe_images() {
        assert!(parse("![x](local.png) ![y](javascript:alert(1))").is_empty());
    }
}
