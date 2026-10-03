use image::RgbaImage;
use serde::Serialize;

use crate::ocr::OcrLine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Heading,
    List,
    Paragraph,
    /// Standalone website/URL labels: preserve their original text and appearance.
    Metadata,
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
    role: Role,
    foreground: [u8; 3],
    background: [u8; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role { Body, Heading, Metadata }

fn standalone_link(text: &str) -> bool {
    let trimmed = text.trim();
    let first = trimmed.split_whitespace().next().unwrap_or("");
    let rest = trimmed[first.len()..].trim_start();
    if first.starts_with("https://") || first.starts_with("http://") || first.starts_with("www.") {
        return rest.is_empty() || rest.starts_with(['›', '>', '»']);
    }
    // A bare domain must occupy the whole row. A URL mentioned inside prose
    // ("visit https://… for details") is still ordinary prose.
    if !rest.is_empty() { return false; }
    let domain = first.trim_end_matches('/');
    let Some((name, suffix)) = domain.rsplit_once('.') else { return false };
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        && ["com", "org", "net", "io", "dev", "edu", "gov", "vn", "jp", "uk", "ai"].contains(&suffix)
}

fn rgb(hex: &str) -> [u8; 3] {
    [1, 3, 5].map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("sampled RGB color"))
}

fn style_boundary(a: &Line, b: &Line) -> bool {
    // Compare strong foreground changes only on a similar background. Code
    // highlighting is excluded by the caller. White-box tests have equal colors.
    dist(a.foreground, b.foreground) > 75 * 75 && dist(a.background, b.background) < 40 * 40
}

fn body_continuation(group: &[Line], line: &Line) -> bool {
    let last = group.last().expect("non-empty group");
    let widest = group.iter().map(|l| l.w).fold(0.0, f64::max);
    !last.is_code && !line.is_code && last.role == Role::Body && line.role == Role::Body
        && !ends_sentence(&last.text) && last.w >= 0.7 * widest
}

fn returns_to_style(lines: &[Line], evidence: usize, reference: &Line) -> bool {
    let mut previous = &lines[evidence];
    for next in lines.iter().skip(evidence + 1).take(3) {
        if next.is_code || next.role == Role::Metadata || starts_list_item(&next.text)
            || ends_sentence(&previous.text) || next.y - previous.bottom() > 0.9 * previous.h
            || (next.x - reference.x).abs() > 1.5 * reference.h {
            break;
        }
        if next.h >= 0.95 * reference.h && next.h <= 1.2 * reference.h && !style_boundary(reference, next) {
            return true;
        }
        previous = next;
    }
    false
}

fn classify_roles(lines: &mut [Line]) {
    for line in lines.iter_mut() {
        if !line.is_code && standalone_link(&line.text) { line.role = Role::Metadata; }
    }
    let mut roles: Vec<Role> = lines.iter().enumerate().map(|(i, line)| {
        if line.is_code || line.role == Role::Metadata || starts_list_item(&line.text) { return line.role; }
        let previous = i.checked_sub(1).map(|j| &lines[j]);
        let independent = previous.is_none_or(|prev| prev.role == Role::Metadata
            || ends_sentence(&prev.text) || line.y - prev.bottom() > 0.6 * prev.h);
        if !independent { return Role::Body; }
        // A title needs corroboration from nearby smaller body text. A taller
        // inline-code row in the middle of a sentence cannot become a heading.
        let title = lines.iter().enumerate().skip(i + 1).take(3).any(|(j, next)| {
            !next.is_code && next.role != Role::Metadata && !starts_list_item(&next.text)
                && next.y >= line.bottom() - 0.35 * line.h && next.y - line.bottom() < 2.5 * line.h
                && (next.x - line.x).abs() < 1.5 * line.h
                && line.h > next.h * 1.12
                && !returns_to_style(lines, j, line)
                && (style_boundary(line, next)
                    || previous.is_some_and(|prev| prev.role == Role::Metadata)
                    || line.h > next.h * 1.5 && line.w < next.w * 0.9)
        });
        if title { Role::Heading } else { Role::Body }
    }).collect();
    // A wrapped heading may span several rows. Propagate the hint through
    // matching title rows, stopping at smaller/differently styled body text.
    for i in 1..lines.len() {
        let (prev, line) = (&lines[i - 1], &lines[i]);
        let gap = line.y - prev.bottom();
        if roles[i - 1] == Role::Heading && roles[i] == Role::Body
            && !line.is_code && !starts_list_item(&line.text) && !ends_sentence(&prev.text)
            && line.h >= 0.9 * prev.h && line.h <= 1.2 * prev.h && !style_boundary(prev, line)
            && gap > -0.35 * prev.h && gap < 0.9 * prev.h
            && ((line.x - prev.x).abs() < 1.5 * prev.h || (line.center() - prev.center()).abs() < 1.5 * prev.h)
        { roles[i] = Role::Heading; }
    }
    for (line, role) in lines.iter_mut().zip(roles) { line.role = role; }
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

/// Why `line` can't join the paragraph `group`, or `None` if it continues it. The text is a
/// diagnostic for debug builds (geometry only, never recognized text).
fn split_reason(group: &[Line], line: &Line) -> Option<String> {
    if group.len() >= MAX_GROUP_LINES {
        return Some(format!("block is full ({MAX_GROUP_LINES} lines)"));
    }
    let last = group.last().expect("non-empty group");
    // Code/terminal lines must never bridge into or out of prose: merging them
    // produced one giant run-on block whose translation was unrelated nonsense.
    if last.is_code != line.is_code {
        return Some(format!("code/prose boundary (previous code={}, this code={})", last.is_code, line.is_code));
    }
    if starts_list_item(&line.text) {
        return Some("starts a list item".into());
    }
    // Priority: structural/style boundaries veto merging. Sentence continuation
    // below can relax size checks only; it cannot override these boundaries.
    if !line.is_code && last.role != line.role {
        return Some(format!("role boundary {:?} -> {:?}", last.role, line.role));
    }
    // Color alone is ambiguous inside a wrapped sentence (e.g. an inline code
    // span). Confirmed role boundaries above still veto body continuation.
    if !line.is_code && line.role != Role::Metadata && style_boundary(last, line)
        && !body_continuation(group, line) {
        return Some("foreground style boundary".into());
    }
    // OCR box height follows glyph content (a line without ascenders/descenders is shorter),
    // so compare against the block's median rather than a single neighbouring line.
    let h = median(group.iter().map(|l| l.h).collect()).max(1.0);
    let gap = line.y - last.bottom();
    if !(gap > -0.35 * h && gap < 0.9 * h) {
        return Some(format!("vertical gap {gap:.1}px = {:.2}h, allowed -0.35h..0.9h (h={h:.1})", gap / h));
    }
    let first = &group[0];
    // Code indentation varies by nesting level (far more than prose margins ever
    // do), so code lines get a much looser left/center/right tolerance.
    let tolerance = if line.is_code { 8.0 } else { 1.5 } * h;
    let aligned = (line.x - first.x).abs() < tolerance
        || (line.center() - first.center()).abs() < tolerance
        || (line.right() - first.right()).abs() < tolerance;
    if !aligned {
        return Some(format!("misaligned: left offset {:.1}px, tolerance {tolerance:.1}px", line.x - first.x));
    }
    // A wrapped line that stops mid-sentence and spans nearly the full width is followed by its
    // own continuation, so tolerate a large height difference there: OCR boxes follow glyph
    // content, and a line made mostly of x-height monospace (an inline code span) can be ~0.6x
    // as tall as its neighbours.
    let mid_sentence = body_continuation(group, line);
    // A lone unpunctuated line is usually a heading, so a much *smaller* next line only joins an
    // established multi-line paragraph.
    let low = if mid_sentence && group.len() >= 2 { 0.45 } else { 0.75 };
    let high = if mid_sentence { 1.8 } else { 1.33 };
    let ratio = line.h / h;
    if !(low..=high).contains(&ratio) {
        return Some(format!("size ratio {ratio:.2} outside {low}..{high} (h={h:.1}, mid_sentence={mid_sentence})"));
    }
    None
}

fn continues(group: &[Line], line: &Line) -> bool {
    split_reason(group, line).is_none()
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end().chars().last().is_some_and(|c| matches!(c, '.' | '?' | '!' | ':' | ';' | '…' | '。' | '！' | '？' | '：' | '；'))
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// Spread (max - min) of the left edges, centers and right edges of `lines`.
fn edge_spreads(lines: &[Line]) -> (f64, f64, f64) {
    let spread = |f: fn(&Line) -> f64| {
        let vals: Vec<f64> = lines.iter().map(f).collect();
        vals.iter().cloned().fold(f64::MIN, f64::max) - vals.iter().cloned().fold(f64::MAX, f64::min)
    };
    (spread(|l| l.x), spread(Line::center), spread(Line::right))
}

fn align_of(lines: &[Line]) -> Align {
    if lines.len() < 2 {
        return Align::Left;
    }
    let h = lines[0].h;
    let (l, c, r) = edge_spreads(lines);
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
    if lines[0].is_code || lines[0].role == Role::Metadata {
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
        .map(|l| {
            let (x, y, w, h) = (l.x * iw, l.y * ih, l.width * iw, l.height * ih);
            let (foreground, background) = sample_colors(image, x, y, w, h);
            Line { text: l.text.clone(), x, y, w, h, is_code: false, role: Role::Body,
                foreground: rgb(&foreground), background: rgb(&background) }
        })
        .collect();
    lines.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
    classify_code(&mut lines);
    classify_roles(&mut lines);

    let mut groups: Vec<Vec<Line>> = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let candidate = (0..groups.len()).rev().take(4).find(|&i| {
            // Looking back supports interleaved columns, but must not jump over
            // a block in the same column, bypassing an earlier split decision.
            let crosses_block = groups[i + 1..].iter().any(|g| g.iter().any(|other|
                other.x < line.right() && other.right() > line.x));
            !crosses_block && continues(&groups[i], &line)
        });
        match candidate {
            Some(i) => {
                if cfg!(debug_assertions) {
                    let last = groups[i].last().unwrap();
                    let h = median(groups[i].iter().map(|l| l.h).collect()).max(1.0);
                    eprintln!("layout: line {index} joins block {i}: role={:?}, code={}, gap={:.2}h, size={:.2}x, color_delta={}",
                        line.role, line.is_code, (line.y - last.bottom()) / h, line.h / h, dist(last.foreground, line.foreground));
                }
                groups[i].push(line);
            }
            None => {
                if cfg!(debug_assertions) {
                    if let Some(reason) = groups.last().and_then(|g| split_reason(g, &line)) {
                        eprintln!("layout: line {index} starts a new block: {reason}");
                    }
                }
                groups.push(vec![line]);
            }
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
            } else if g[0].role == Role::Metadata {
                Kind::Metadata
            } else if starts_list_item(&g[0].text) {
                Kind::List
            } else if g[0].role == Role::Heading || g.len() <= 2 && line_height > 1.3 * median_h {
                Kind::Heading
            } else {
                Kind::Paragraph
            };
            let align = align_of(&g);
            if cfg!(debug_assertions) && g.len() > 1 {
                let (l, c, r) = edge_spreads(&g);
                eprintln!(
                    "layout: block {:?} lines={} align={align:?} spreads left={l:.1} center={c:.1} right={r:.1} h={:.1}px",
                    kind, g.len(), g[0].h
                );
            }
            let (color, background) = sample_colors(image, x, y, right - x, bottom - y);
            Block {
                x,
                y,
                width: right - x,
                height: bottom - y,
                line_height,
                line_count: g.len(),
                kind,
                align,
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
    fn real_search_results_keep_metadata_titles_and_snippets_separate() {
        let lines: Vec<OcrLine> = serde_json::from_str(include_str!("../assets/ocr-search-results.json")).unwrap();
        let image = image::load_from_memory(include_bytes!("../assets/ocr-search-results.png")).unwrap().to_rgba8();
        let blocks = build_blocks(&lines, &image);
        let summary: Vec<_> = blocks.iter().map(|b| (b.kind, b.line_count)).collect();
        assert_eq!(summary, [
            (Kind::Metadata, 2), (Kind::Heading, 1), (Kind::Paragraph, 2),
            (Kind::Metadata, 2), (Kind::Heading, 1), (Kind::Paragraph, 2),
        ]);
        assert!(blocks[0].text.contains("https://"));
        assert!(!blocks[2].text.contains("Drupal Releases"));
        assert!(!blocks[5].text.contains("https://"));
        assert_ne!(blocks[1].color, blocks[2].color);
        assert_ne!(blocks[4].color, blocks[5].color);
        assert!(blocks[1].line_height > blocks[2].line_height);
        assert!(blocks[4].line_height > blocks[5].line_height);
        eprintln!("search layout preview: {}", serde_json::to_string(&blocks).unwrap());
    }

    #[test]
    fn a_link_between_body_rows_cannot_be_bypassed_by_lookback() {
        let lines = [
            line("This unfinished body row needs its own block", 10.0, 10.0, 700.0, 40.0),
            line("https://example.com", 10.0, 52.0, 180.0, 8.0),
            line("Another body row after a link", 10.0, 64.0, 700.0, 40.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].kind, Kind::Metadata);
        assert!(blocks.iter().all(|b| b.line_count == 1));
    }

    #[test]
    fn interleaved_columns_still_join_within_their_own_column() {
        let lines = [
            line("Left column starts with", 10.0, 10.0, 250.0, 20.0),
            line("Right column starts with", 600.0, 11.0, 250.0, 20.0),
            line("its own continuation", 10.0, 34.0, 220.0, 20.0),
            line("a separate continuation", 600.0, 35.0, 230.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 2);
        assert!(blocks.iter().all(|b| b.line_count == 2));
    }

    #[test]
    fn a_url_at_the_start_of_a_sentence_is_still_translatable_prose() {
        let lines = [
            line("https://example.com provides useful background on this topic", 10.0, 10.0, 800.0, 20.0),
            line("and the rest of this sentence explains its purpose", 10.0, 34.0, 750.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, Kind::Paragraph);
        assert_eq!(blocks[0].line_count, 2);
    }

    #[test]
    fn wrapped_headings_join_without_swallowing_the_body() {
        let lines = [
            line("A long title that wraps onto", 10.0, 10.0, 700.0, 40.0),
            line("a second title row", 10.0, 55.0, 400.0, 40.0),
            line("The smaller body starts here and", 10.0, 105.0, 900.0, 20.0),
            line("continues onto another row.", 10.0, 130.0, 800.0, 20.0),
        ];
        let blocks = build_blocks(&lines, &white());
        assert_eq!(blocks.len(), 2);
        assert_eq!((blocks[0].kind, blocks[0].line_count), (Kind::Heading, 2));
        assert_eq!((blocks[1].kind, blocks[1].line_count), (Kind::Paragraph, 2));
    }

    #[test]
    fn colored_inline_code_and_short_boxes_still_continue_the_body() {
        let lines = [
            line("The Windows OCR test recognizes the bundled image and", 10.0, 10.0, 800.0, 20.0),
            line("then run this command and check the output from", 10.0, 33.0, 790.0, 20.0),
            line("test --manifest-path src-tauri/Cargo.toml --test desktop japanese", 10.0, 58.0, 810.0, 12.0),
            line("the fallback before saving another translated image", 10.0, 76.0, 800.0, 20.0),
        ];
        let mut image = white();
        for (i, row) in lines.iter().enumerate() {
            let color = if i == 2 { [140, 20, 170, 255] } else { [70, 70, 70, 255] };
            for y in (row.y * 1000.0) as u32 + 2..((row.y + row.height) * 1000.0) as u32 - 2 {
                for x in (row.x * 1000.0) as u32 + 2..((row.x + row.width) * 1000.0) as u32 - 2 {
                    image.put_pixel(x, y, Rgba(color));
                }
            }
        }
        let blocks = build_blocks(&lines, &image);
        assert_eq!(blocks.len(), 1);
        assert_eq!((blocks[0].kind, blocks[0].line_count), (Kind::Paragraph, 4));
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


    /// Real Vision output for a 13-line wrapped paragraph (889×325): box heights vary from
    /// 0.049 to 0.062 depending on glyph content, which must not break the paragraph apart.
    #[test]
    fn wrapped_paragraph_with_varying_box_heights_stays_one_block() {
        let rows: [(f64, f64, f64, f64); 13] = [
            (0.021, 0.058, 0.021, 0.863), (0.104, 0.055, 0.022, 0.919), (0.170, 0.057, 0.029, 0.883),
            (0.252, 0.055, 0.025, 0.937), (0.325, 0.049, 0.022, 0.888), (0.392, 0.062, 0.022, 0.942),
            (0.472, 0.055, 0.022, 0.946), (0.546, 0.061, 0.025, 0.939), (0.620, 0.055, 0.025, 0.908),
            (0.681, 0.058, 0.021, 0.804), (0.761, 0.062, 0.029, 0.917), (0.840, 0.055, 0.025, 0.928),
            (0.920, 0.049, 0.025, 0.346),
        ];
        let lines: Vec<OcrLine> = rows
            .iter()
            .enumerate()
            .map(|(i, &(y, height, x, width))| OcrLine { text: format!("word{i} continues the sentence without a stop"), x, y, width, height })
            .collect();
        let image = RgbaImage::from_pixel(889, 325, Rgba([30, 33, 40, 255]));
        assert_eq!(build_blocks(&lines, &image).len(), 1);
    }

    #[test]
    fn taller_line_in_the_middle_of_a_sentence_does_not_split_the_paragraph() {
        // A line holding an inline code span can be ~1.4x taller than its neighbours.
        let lines = [
            line("The worker keeps the active model in memory and", 10.0, 10.0, 600.0, 20.0),
            line("restarts after errors or a long timeout while the", 10.0, 33.0, 600.0, 28.0),
            line("runtime directory holds the logs for each run", 10.0, 66.0, 600.0, 20.0),
        ];
        assert_eq!(build_blocks(&lines, &white()).len(), 1);
    }

    #[test]
    fn short_box_from_an_inline_code_line_does_not_split_a_wrapped_paragraph() {
        // Observed: a line made mostly of x-height monospace text got a box 0.6x as tall.
        let lines = [
            line("The Windows OCR test recognizes the bundled warm-up image and", 10.0, 10.0, 800.0, 20.0),
            line("then run the regression with the command below and check that", 10.0, 33.0, 790.0, 20.0),
            line("test --manifest-path src-tauri/Cargo.toml --test desktop japanese", 10.0, 58.0, 810.0, 12.0),
            line("the fallback is used when no language pack is installed on this", 10.0, 76.0, 800.0, 20.0),
        ];
        assert_eq!(build_blocks(&lines, &white()).len(), 1);
    }

    #[test]
    fn size_change_after_a_finished_sentence_still_splits() {
        let lines = [
            line("The first paragraph ends here.", 10.0, 10.0, 600.0, 20.0),
            line("Next heading-sized text", 10.0, 33.0, 600.0, 36.0),
        ];
        assert_eq!(build_blocks(&lines, &white()).len(), 2);
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
