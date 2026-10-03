use image::RgbaImage;
use serde::Serialize;

use crate::ocr::OcrLine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Heading,
    List,
    Paragraph,
    /// Source code or terminal output: lines are never merged with surrounding
    /// prose, and merged runs are capped (see `MAX_GROUP_LINES`) so a mis-detected
    /// run can't swallow unrelated content into one block.
    Code,
}

/// Hard cap on lines per block, independent of the heuristics below: bounds the
/// damage from a misdetected merge (translating/rendering one giant run-on block).
const MAX_GROUP_LINES: usize = 40;

/// Common source/terminal tokens. OCR doesn't report leading indentation as
/// literal spaces (that's encoded in the line's `x`, not its text), so this is
/// content-only and deliberately conservative: a false negative just falls back
/// to paragraph grouping, but a false positive would wrongly block legitimate
/// prose from joining its neighbors.
fn looks_like_code(text: &str) -> bool {
    let t = text.trim();
    // A line that is nothing but brackets/separators (e.g. a lone closing `}` or
    // `);`) is never ordinary prose on its own.
    if !t.is_empty() && t.chars().all(|c| matches!(c, '{' | '}' | '(' | ')' | '[' | ']' | ';' | ',')) {
        return true;
    }
    // Whole-word only: substring matching on "use "/"let "/"return "/"pub " also
    // hits "because", "house", "let us", "point of return", "pub quiz", etc.
    // Kept to symbols that essentially never appear inside English words.
    const KEYWORDS: [&str; 4] = ["fn", "impl", "struct", "enum"];
    if text.split_whitespace().any(|w| KEYWORDS.contains(&w.trim_matches(|c: char| !c.is_alphanumeric()))) {
        return true;
    }
    const SYMBOLS: [&str; 7] = ["=>", "->", "::", "&&", "||", "#[", "/*"];
    if SYMBOLS.iter().any(|s| text.contains(s)) {
        return true;
    }
    // "//" is a strong comment marker, but also appears in "https://..."; only
    // count it when it's not immediately preceded by a URL scheme's colon.
    if let Some(pos) = text.find("//") {
        if !text[..pos].ends_with(':') {
            return true;
        }
    }
    // Dense punctuation (braces/semicolons/parens) relative to letters, e.g. `} else {`.
    let brackets = text.chars().filter(|c| matches!(c, '{' | '}' | ';' | '(' | ')' | '[' | ']')).count();
    let alpha = text.chars().filter(|c| c.is_alphabetic()).count();
    brackets >= 2 && brackets * 2 >= alpha
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A paragraph-like group of lines. Geometry is in image pixels.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Median line height in pixels; approximates the rendered font size.
    pub line_height: f64,
    pub line_count: usize,
    pub kind: Kind,
    pub align: Align,
    pub color: String,
    pub background: String,
    pub text: String,
}

#[derive(Debug, Clone)]
struct Line {
    text: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// Precomputed by `classify_code` over the whole line sequence (context-aware,
    /// unlike `looks_like_code` on its own).
    is_code: bool,
}

impl Line {
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn center(&self) -> f64 {
        self.x + self.w / 2.0
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
}

fn starts_list_item(text: &str) -> bool {
    let t = text.trim_start();
    let mut chars = t.chars();
    match chars.next() {
        // OCR often attaches a Japanese bullet directly to the first glyph.
        // Keep ordinary hyphens/asterisks conservative (e.g. -1, *pointer).
        Some('•' | '·' | '・' | '▪' | '◦' | '‣' | '●' | '○' | '■') => {
            chars.next().is_some()
        }
        Some('–' | '-' | '*') => {
            chars.next().is_some_and(char::is_whitespace)
        }
        Some(c) if c.is_ascii_digit() => {
            let rest = t.trim_start_matches(|c: char| c.is_ascii_digit());
            rest.starts_with(". ") || rest.starts_with(") ")
        }
        _ => false,
    }
}

/// Whether `line` continues the paragraph whose lines are `group`.
fn continues(group: &[Line], line: &Line) -> bool {
    if group.len() >= MAX_GROUP_LINES {
        return false;
    }
    let last = group.last().expect("non-empty group");
    // Code/terminal lines must never bridge into or out of prose: merging them
    // produced one giant run-on block whose translation was unrelated nonsense.
    if last.is_code != line.is_code {
        return false;
    }
    let h = last.h.max(1.0);
    let similar_size = (0.75..=1.33).contains(&(line.h / h));
    let gap = line.y - last.bottom();
    let close_below = gap > -0.35 * h && gap < 0.9 * h;
    let first = &group[0];
    // Code indentation varies by nesting level (far more than prose margins ever
    // do), so code lines get a much looser left/center/right tolerance.
    let tolerance = if line.is_code { 8.0 } else { 1.5 } * h;
    let aligned = (line.x - first.x).abs() < tolerance
        || (line.center() - first.center()).abs() < tolerance
        || (line.right() - first.right()).abs() < tolerance;
    similar_size && close_below && aligned && !starts_list_item(&line.text)
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn align_of(lines: &[Line]) -> Align {
    if lines.len() < 2 {
        return Align::Left;
    }
    let spread = |f: fn(&Line) -> f64| {
        let vals: Vec<f64> = lines.iter().map(f).collect();
        vals.iter().cloned().fold(f64::MIN, f64::max) - vals.iter().cloned().fold(f64::MAX, f64::min)
    };
    let h = lines[0].h;
    let (l, c, r) = (spread(|l| l.x), spread(Line::center), spread(Line::right));
    if l <= c && l <= r || l < 0.5 * h {
        Align::Left
    } else if c <= r {
        Align::Center
    } else {
        Align::Right
    }
}

/// Prose is reflowed into a single line (translators expect that); code/terminal
/// output keeps its line breaks since each line is usually its own statement.
fn join_lines(lines: &[Line]) -> String {
    if lines[0].is_code {
        return lines.iter().map(|l| l.text.trim_end()).collect::<Vec<_>>().join("\n");
    }
    let mut out = String::new();
    for line in lines {
        let t = line.text.trim();
        if out.ends_with('-') && !out.ends_with(" -") {
            out.pop();
        } else if !out.is_empty() && !is_cjk(out.chars().last()) && !is_cjk(t.chars().next()) {
            out.push(' ');
        }
        out.push_str(t);
    }
    out
}

/// `looks_like_code` alone misses plain statements with no distinctive symbol
/// (e.g. `let x = compute();` has only one `;`). A line with no signal of its own
/// but sandwiched tightly between two already-code lines at a similar indent
/// (same shape as the rest of a function body) is still code.
fn classify_code(lines: &mut [Line]) {
    for line in lines.iter_mut() {
        line.is_code = looks_like_code(&line.text);
    }
    // Only the vertical gap matters here: nested code is commonly indented further
    // right than its `fn`/`{` line, so an x-proximity check would defeat the very
    // case this is meant to catch.
    let tight = |a: &Line, b: &Line| {
        let h = a.h.max(1.0);
        let gap = b.y - a.bottom();
        gap > -0.35 * h && gap < 0.9 * h
    };
    for i in 1..lines.len().saturating_sub(1) {
        if !lines[i].is_code
            && lines[i - 1].is_code
            && lines[i + 1].is_code
            && tight(&lines[i - 1], &lines[i])
            && tight(&lines[i], &lines[i + 1])
        {
            lines[i].is_code = true;
        }
    }
}

fn is_cjk(c: Option<char>) -> bool {
    c.is_some_and(|c| matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF))
}

/// Groups OCR lines into blocks in reading order and samples their colors.
pub fn build_blocks(lines: &[OcrLine], image: &RgbaImage) -> Vec<Block> {
    let (iw, ih) = (f64::from(image.width()), f64::from(image.height()));
    let mut lines: Vec<Line> = lines
        .iter()
        .map(|l| Line { text: l.text.clone(), x: l.x * iw, y: l.y * ih, w: l.width * iw, h: l.height * ih, is_code: false })
        .collect();
    lines.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    classify_code(&mut lines);

    let mut groups: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        match groups.iter_mut().rev().take(4).find(|g| continues(g, &line)) {
            Some(group) => group.push(line),
            None => groups.push(vec![line]),
        }
    }

    let median_h = if groups.is_empty() {
        0.0
    } else {
        median(groups.iter().flatten().map(|l| l.h).collect())
    };

    groups
        .into_iter()
        .map(|g| {
            let x = g.iter().map(|l| l.x).fold(f64::MAX, f64::min);
            let y = g.iter().map(|l| l.y).fold(f64::MAX, f64::min);
            let right = g.iter().map(Line::right).fold(f64::MIN, f64::max);
            let bottom = g.iter().map(Line::bottom).fold(f64::MIN, f64::max);
            let line_height = median(g.iter().map(|l| l.h).collect());
            let kind = if g[0].is_code {
                Kind::Code
            } else if starts_list_item(&g[0].text) {
                Kind::List
            } else if g.len() <= 2 && line_height > 1.3 * median_h {
                Kind::Heading
            } else {
                Kind::Paragraph
            };
            let (color, background) = sample_colors(image, x, y, right - x, bottom - y);
            Block {
                x,
                y,
                width: right - x,
                height: bottom - y,
                line_height,
                line_count: g.len(),
                kind,
                align: align_of(&g),
                color,
                background,
                text: join_lines(&g),
            }
        })
        .collect()
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn dist(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter().zip(b).map(|(&x, y)| (i32::from(x) - i32::from(y)).unsigned_abs().pow(2)).sum()
}

/// Background = per-channel median of a ring just outside the box; text = mean of the
/// pixels inside the box that differ most from the background.
fn sample_colors(image: &RgbaImage, x: f64, y: f64, w: f64, h: f64) -> (String, String) {
    let (iw, ih) = (image.width() as i64, image.height() as i64);
    let pad = 3i64;
    let x0 = (x as i64 - pad).clamp(0, iw - 1);
    let y0 = (y as i64 - pad).clamp(0, ih - 1);
    let x1 = ((x + w) as i64 + pad).clamp(0, iw - 1);
    let y1 = ((y + h) as i64 + pad).clamp(0, ih - 1);
    let px = |x: i64, y: i64| {
        let p = image.get_pixel(x as u32, y as u32).0;
        [p[0], p[1], p[2]]
    };

    let mut ring = Vec::new();
    for xx in x0..=x1 {
        ring.push(px(xx, y0));
        ring.push(px(xx, y1));
    }
    for yy in y0..=y1 {
        ring.push(px(x0, yy));
        ring.push(px(x1, yy));
    }
    let channel_median = |c: usize| {
        let mut v: Vec<u8> = ring.iter().map(|p| p[c]).collect();
        v.sort_unstable();
        v[v.len() / 2]
    };
    let bg = [channel_median(0), channel_median(1), channel_median(2)];

    let inner: Vec<[u8; 3]> = (y0..=y1).flat_map(|yy| (x0..=x1).map(move |xx| (xx, yy))).map(|(a, b)| px(a, b)).collect();
    let max = inner.iter().map(|&p| dist(p, bg)).max().unwrap_or(0);
    let fg = if max == 0 {
        if bg.iter().map(|&c| u32::from(c)).sum::<u32>() > 384 { [0, 0, 0] } else { [255, 255, 255] }
    } else {
        let strong: Vec<_> = inner.iter().filter(|&&p| dist(p, bg) * 2 >= max).collect();
        let n = strong.len() as u32;
        let mean = |c: usize| (strong.iter().map(|p| u32::from(p[c])).sum::<u32>() / n) as u8;
        [mean(0), mean(1), mean(2)]
    };
    (hex(fg), hex(bg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn line(text: &str, x: f64, y: f64, w: f64, h: f64) -> OcrLine {
        // Normalized against a 1000×1000 image.
        OcrLine { text: text.into(), x: x / 1000.0, y: y / 1000.0, width: w / 1000.0, height: h / 1000.0 }
    }

    fn white() -> RgbaImage {
        RgbaImage::from_pixel(1000, 1000, Rgba([255, 255, 255, 255]))
    }

    #[test]
    fn groups_paragraph_lines_and_splits_on_gaps_headings_and_bullets() {
        let lines = [
            line("Big Title", 10.0, 10.0, 300.0, 40.0),
            line("First line of a", 10.0, 80.0, 400.0, 20.0),
            line("paragraph here.", 10.0, 104.0, 300.0, 20.0),
            line("• item one", 10.0, 140.0, 200.0, 20.0),
            line("• item two", 10.0, 164.0, 200.0, 20.0),
            line("Far away text", 10.0, 400.0, 200.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        let summary: Vec<_> = blocks.iter().map(|b| (b.kind, b.text.as_str())).collect();
        assert_eq!(
            summary,
            [
                (Kind::Heading, "Big Title"),
                (Kind::Paragraph, "First line of a paragraph here."),
                (Kind::List, "• item one"),
                (Kind::List, "• item two"),
                (Kind::Paragraph, "Far away text"),
            ]
        );
        assert_eq!((blocks[1].y, blocks[1].height, blocks[1].width), (80.0, 44.0, 400.0));
    }

    #[test]
    fn japanese_bullets_without_spaces_remain_separate_list_items() {
        let lines = [
            line("データ加工・整理", 300.0, 10.0, 400.0, 30.0),
            line("·弊社側で保持していない項目の付加", 30.0, 80.0, 500.0, 20.0),
            line("・分類／名寄せ、集計軸の整備", 30.0, 108.0, 450.0, 20.0),
            line("•現データと過去データの統合", 30.0, 136.0, 470.0, 20.0),
            line("·分析要件に合わせた加工", 30.0, 164.0, 400.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 5);
        assert!(blocks[1..].iter().all(|b| b.kind == Kind::List && b.line_count == 1));
        assert_eq!(blocks[4].y, 164.0);
        assert!(!starts_list_item("-1"));
        assert!(!starts_list_item("*pointer"));
    }

    #[test]
    fn side_by_side_columns_stay_separate() {
        let lines = [line("left column", 10.0, 10.0, 200.0, 20.0), line("right column", 600.0, 12.0, 200.0, 20.0)];
        assert_eq!(build_blocks(&lines, &white()).len(), 2);
    }

    #[test]
    fn detects_centered_alignment() {
        let lines = [line("a much longer centered line", 100.0, 10.0, 800.0, 20.0), line("short", 400.0, 34.0, 200.0, 20.0)];
        assert_eq!(build_blocks(&lines, &white())[0].align, Align::Center);
    }

    #[test]
    fn caps_block_at_max_group_lines() {
        let lines: Vec<OcrLine> =
            (0..MAX_GROUP_LINES + 10).map(|i| line(&format!("row {i}"), 10.0, 10.0 + i as f64 * 24.0, 200.0, 20.0)).collect();
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks[0].line_count, MAX_GROUP_LINES);
        assert_eq!(blocks[1].line_count, 10);
    }

    #[test]
    fn code_lines_form_their_own_block_separate_from_surrounding_prose() {
        let lines = [
            line("Explanation: run the function below to see results", 10.0, 10.0, 500.0, 20.0),
            line("fn main() {", 10.0, 42.0, 150.0, 20.0),
            line("println!(\"{}\", x);", 10.0, 66.0, 220.0, 20.0),
            line("That concludes the demo for today", 10.0, 98.0, 400.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        let summary: Vec<_> = blocks.iter().map(|b| (b.kind, b.text.as_str())).collect();
        assert_eq!(
            summary,
            [
                (Kind::Paragraph, "Explanation: run the function below to see results"),
                (Kind::Code, "fn main() {\nprintln!(\"{}\", x);"),
                (Kind::Paragraph, "That concludes the demo for today"),
            ]
        );
    }

    /// Matches the real OCR output that exposed the gap: `let x = compute();` has
    /// no distinctive symbol of its own (one `;`, no keyword, no brackets), so it
    /// must be inferred from sitting between two already-code lines.
    #[test]
    fn code_line_with_no_own_signal_is_smoothed_between_code_neighbors() {
        let lines = [
            line("fn main() {", 10.0, 10.0, 150.0, 20.0),
            line("let x = compute();", 30.0, 34.0, 220.0, 20.0),
            line("println!(\"{}\", x);", 30.0, 58.0, 230.0, 20.0),
            line("}", 10.0, 82.0, 20.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, Kind::Code);
        assert_eq!(blocks[0].text, "fn main() {\nlet x = compute();\nprintln!(\"{}\", x);\n}");
    }

    /// Real terminals/editors indent a function body well past ordinary prose's
    /// margin tolerance (here 2.4h, matching a 4-space monospace indent); code
    /// groups must use a looser alignment check than prose does.
    #[test]
    fn code_block_merges_across_a_deep_indent() {
        let lines = [
            line("fn main() {", 10.0, 10.0, 150.0, 20.0),
            line("let x = compute();", 58.0, 34.0, 220.0, 20.0),
            line("println!(\"{}\", x);", 58.0, 58.0, 230.0, 20.0),
            line("}", 10.0, 82.0, 20.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, Kind::Code);
        assert_eq!(blocks[0].text, "fn main() {\nlet x = compute();\nprintln!(\"{}\", x);\n}");
    }

    /// Regression: naive substring matching on code keywords ("use ", "let ", "return ",
    /// "//") previously matched inside ordinary English ("because", "let us", a URL),
    /// splitting normal paragraphs into several blocks.
    #[test]
    fn prose_mentioning_code_like_words_and_a_url_stays_one_paragraph() {
        let lines = [
            line("This article happened because of complications in logistics", 10.0, 10.0, 600.0, 20.0),
            line("visit https://example.com/info for more context on the matter", 10.0, 43.0, 600.0, 20.0),
            line("and let us consider the return value of this function call", 10.0, 76.0, 600.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, Kind::Paragraph);
        assert_eq!(
            blocks[0].text,
            "This article happened because of complications in logistics \
             visit https://example.com/info for more context on the matter \
             and let us consider the return value of this function call"
        );
    }

    #[test]
    fn samples_text_and_background_colors() {
        let mut img = RgbaImage::from_pixel(100, 100, Rgba([20, 30, 40, 255]));
        for x in 30..60 {
            for y in 40..50 {
                img.put_pixel(x, y, Rgba([250, 200, 0, 255]));
            }
        }
        let (fg, bg) = sample_colors(&img, 25.0, 38.0, 40.0, 14.0);
        assert_eq!((fg.as_str(), bg.as_str()), ("#fac800", "#141e28"));
    }
}
