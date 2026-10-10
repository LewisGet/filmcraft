//! Bounded SVG 1.1 subset model, parser, serializer and CPU rasterizer.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

use filmcraft_text::{Path, mask, raster};
use roxmltree::{Document, Node, ParsingOptions};
use serde::{Deserialize, Serialize};

pub const MAX_SVG_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ELEMENTS: usize = 20_000;
pub const MAX_DEPTH: usize = 64;
pub const MAX_PATH_COMMANDS: usize = 100_000;
pub const MAX_CANVAS_SIDE: u32 = 16_384;
pub const MAX_CANVAS_PIXELS: u64 = 67_108_864;

#[derive(Debug, thiserror::Error)]
pub enum SvgError {
    #[error("SVG exceeds the {MAX_SVG_BYTES}-byte input limit")]
    InputTooLarge,
    #[error("invalid SVG: {0}")]
    Parse(String),
    #[error("invalid SVG dimensions")]
    InvalidDimensions,
    #[error("SVG canvas exceeds supported raster limits")]
    CanvasTooLarge,
    #[error("SVG path exceeds supported command limits")]
    PathTooLarge,
}

pub type Result<T> = std::result::Result<T, SvgError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Default for Color {
    fn default() -> Self { Self(255, 255, 255, 255) }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rect { x: f32, y: f32, width: f32, height: f32 },
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    Path { commands: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub kind: ShapeKind,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SvgDocument {
    pub width: u32,
    pub height: u32,
    pub shapes: Vec<Shape>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvgWarning(pub String);

impl Default for SvgDocument {
    fn default() -> Self { Self { width: 1920, height: 1080, shapes: Vec::new() } }
}

impl SvgDocument {
    pub fn parse(source: &str) -> Result<(Self, Vec<SvgWarning>)> {
        if source.len() > MAX_SVG_BYTES { return Err(SvgError::InputTooLarge); }
        let options = ParsingOptions { allow_dtd: false, nodes_limit: MAX_ELEMENTS as u32, ..ParsingOptions::default() };
        let xml = Document::parse_with_options(source, options).map_err(|e| SvgError::Parse(e.to_string()))?;
        let root = xml.root_element();
        if root.tag_name().name() != "svg" { return Err(SvgError::Parse("root element must be <svg>".into())); }
        let (width, height) = dimensions(root)?;
        validate_dimensions(width, height)?;
        let mut document = Self { width, height, shapes: Vec::new() };
        let mut warnings = Vec::new();
        let mut visited = 0usize;
        parse_children(root, 0, &mut visited, &mut document.shapes, &mut warnings)?;
        Ok((document, warnings))
    }

    pub fn to_svg(&self) -> Result<String> {
        validate_dimensions(self.width, self.height)?;
        if self.shapes.len() > MAX_ELEMENTS { return Err(SvgError::Parse("too many shapes".into())); }
        let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">", self.width, self.height, self.width, self.height);
        for shape in &self.shapes {
            let geometry = match &shape.kind {
                ShapeKind::Rect { x, y, width, height } => format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", number(*x)?, number(*y)?, number(*width)?, number(*height)?),
                ShapeKind::Ellipse { cx, cy, rx, ry } if (*rx - *ry).abs() < f32::EPSILON => format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\"", number(*cx)?, number(*cy)?, number(*rx)?),
                ShapeKind::Ellipse { cx, cy, rx, ry } => format!("<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"", number(*cx)?, number(*cy)?, number(*rx)?, number(*ry)?),
                ShapeKind::Path { commands } => format!("<path d=\"{}\"", escape_attr(commands)),
            };
            out.push_str(&geometry);
            out.push_str(&format!(" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" opacity=\"{}\"/>", color_text(shape.fill), color_text(shape.stroke), number(shape.stroke_width)?, number(shape.opacity)?));
        }
        out.push_str("</svg>");
        Ok(out)
    }

    pub fn rasterize(&self) -> Result<Vec<u8>> {
        validate_dimensions(self.width, self.height)?;
        let w = self.width as usize;
        let h = self.height as usize;
        let byte_len = w.checked_mul(h).and_then(|v| v.checked_mul(4)).ok_or(SvgError::CanvasTooLarge)?;
        let mut rgba = vec![0u8; byte_len];
        for shape in &self.shapes {
            let path = shape_path(shape)?;
            let fill_mask = raster::fill(&path, w, h, 0.0, 0.0);
            if let Some(color) = shape.fill { composite(&mut rgba, &fill_mask.a, color, shape.opacity); }
            if let Some(color) = shape.stroke.filter(|_| shape.stroke_width > 0.0) {
                let stroked = mask::stroke(&fill_mask, shape.stroke_width, filmcraft_text::StrokeKind::Center);
                composite(&mut rgba, &stroked.a, color, shape.opacity);
            }
        }
        Ok(rgba)
    }
}

fn dimensions(root: Node<'_, '_>) -> Result<(u32, u32)> {
    let width = root.attribute("width").and_then(parse_length).or_else(|| root.attribute("viewBox").and_then(parse_viewbox).map(|v| v.2)).unwrap_or(1920.0);
    let height = root.attribute("height").and_then(parse_length).or_else(|| root.attribute("viewBox").and_then(parse_viewbox).map(|v| v.3)).unwrap_or(1080.0);
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 || width.fract() != 0.0 || height.fract() != 0.0 { return Err(SvgError::InvalidDimensions); }
    Ok((width as u32, height as u32))
}

fn validate_dimensions(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 { return Err(SvgError::InvalidDimensions); }
    if w > MAX_CANVAS_SIDE || h > MAX_CANVAS_SIDE || u64::from(w).checked_mul(u64::from(h)).is_none_or(|n| n > MAX_CANVAS_PIXELS) { return Err(SvgError::CanvasTooLarge); }
    Ok(())
}

fn parse_children(parent: Node<'_, '_>, depth: usize, visited: &mut usize, shapes: &mut Vec<Shape>, warnings: &mut Vec<SvgWarning>) -> Result<()> {
    if depth > MAX_DEPTH { return Err(SvgError::Parse("element nesting exceeds limit".into())); }
    for node in parent.children().filter(|child| child.is_element()) {
        *visited = visited.saturating_add(1);
        if *visited > MAX_ELEMENTS { return Err(SvgError::Parse("element count exceeds limit".into())); }
        let tag = node.tag_name().name();
        if tag == "g" || tag == "svg" {
            if node.attribute("transform").is_some() { warnings.push(SvgWarning("transform is not supported yet and was ignored".into())); }
            parse_children(node, depth + 1, visited, shapes, warnings)?;
            continue;
        }
        if matches!(tag, "image" | "use" | "foreignObject" | "script") {
            warnings.push(SvgWarning(format!("<{tag}> is unsupported and was ignored")));
            continue;
        }
        let kind = match tag {
            "rect" => Some(ShapeKind::Rect { x: attr_num(node, "x", 0.0)?, y: attr_num(node, "y", 0.0)?, width: attr_num(node, "width", 0.0)?, height: attr_num(node, "height", 0.0)? }),
            "circle" => { let r = attr_num(node, "r", 0.0)?; Some(ShapeKind::Ellipse { cx: attr_num(node, "cx", 0.0)?, cy: attr_num(node, "cy", 0.0)?, rx: r, ry: r }) },
            "ellipse" => Some(ShapeKind::Ellipse { cx: attr_num(node, "cx", 0.0)?, cy: attr_num(node, "cy", 0.0)?, rx: attr_num(node, "rx", 0.0)?, ry: attr_num(node, "ry", 0.0)? }),
            "line" => Some(ShapeKind::Path { commands: format!("M {} {} L {} {}", attr_num(node, "x1", 0.0)?, attr_num(node, "y1", 0.0)?, attr_num(node, "x2", 0.0)?, attr_num(node, "y2", 0.0)?) }),
            "polyline" | "polygon" => {
                let points = node.attribute("points").unwrap_or("");
                let nums = parse_numbers(points)?;
                if nums.len() < 4 || nums.len() % 2 != 0 { return Err(SvgError::Parse("invalid points attribute".into())); }
                let mut d = format!("M {} {}", nums[0], nums[1]);
                for pair in nums[2..].chunks_exact(2) { d.push_str(&format!(" L {} {}", pair[0], pair[1])); }
                if tag == "polygon" { d.push_str(" Z"); }
                Some(ShapeKind::Path { commands: d })
            }
            "path" => Some(ShapeKind::Path { commands: node.attribute("d").unwrap_or("").to_string() }),
            _ => { warnings.push(SvgWarning(format!("<{tag}> is unsupported and was ignored"))); None }
        };
        if let Some(kind) = kind {
            let fill = parse_color(node.attribute("fill").unwrap_or("black"))?;
            let stroke = parse_color(node.attribute("stroke").unwrap_or("none"))?;
            let opacity = attr_num(node, "opacity", 1.0)?.clamp(0.0, 1.0);
            let stroke_width = attr_num(node, "stroke-width", 1.0)?.max(0.0);
            shapes.push(Shape { kind, fill, stroke, stroke_width, opacity });
        }
    }
    Ok(())
}

fn shape_path(shape: &Shape) -> Result<Path> {
    match &shape.kind {
        ShapeKind::Rect { x, y, width, height } => Ok(Path::rect(*x, *y, *x + *width, *y + *height)),
        ShapeKind::Ellipse { cx, cy, rx, ry } => Ok(Path::ellipse(*cx, *cy, *rx, *ry)),
        ShapeKind::Path { commands } => parse_path(commands),
    }
}

fn parse_path(data: &str) -> Result<Path> {
    let tokens = tokenize_path(data)?;
    let mut path = Path::new();
    let mut i = 0usize;
    let mut command = 'M';
    let mut current = (0.0f32, 0.0f32);
    let mut start = current;
    let mut count = 0usize;
    while i < tokens.len() {
        if let Some(Some(c)) = tokens.get(i).map(|t| t.chars().next().filter(|c| c.is_ascii_alphabetic())) { command = c; i += 1; }
        count = count.saturating_add(1);
        if count > MAX_PATH_COMMANDS { return Err(SvgError::PathTooLarge); }
        let relative = command.is_ascii_lowercase();
        let upper = command.to_ascii_uppercase();
        let read = |idx: &mut usize| -> Result<f32> { let token = tokens.get(*idx).ok_or_else(|| SvgError::Parse("truncated path command".into()))?; *idx += 1; let value = token.parse::<f32>().map_err(|_| SvgError::Parse("invalid path number".into()))?; if value.is_finite() { Ok(value) } else { Err(SvgError::Parse("non-finite path number".into())) } };
        match upper {
            'Z' => { path.close(); current = start; command = 'M'; }
            'M' | 'L' => { let mut x = read(&mut i)?; let mut y = read(&mut i)?; if relative { x += current.0; y += current.1; } if upper == 'M' { path.move_to(x, y); start = (x, y); command = if relative { 'l' } else { 'L' }; } else { path.line_to(x, y); } current = (x, y); }
            'H' => { let mut x = read(&mut i)?; if relative { x += current.0; } path.line_to(x, current.1); current.0 = x; }
            'V' => { let mut y = read(&mut i)?; if relative { y += current.1; } path.line_to(current.0, y); current.1 = y; }
            'Q' => { let mut x1 = read(&mut i)?; let mut y1 = read(&mut i)?; let mut x = read(&mut i)?; let mut y = read(&mut i)?; if relative { x1 += current.0; y1 += current.1; x += current.0; y += current.1; } path.quad_to(x1, y1, x, y); current = (x, y); }
            'C' => { let mut x1 = read(&mut i)?; let mut y1 = read(&mut i)?; let mut x2 = read(&mut i)?; let mut y2 = read(&mut i)?; let mut x = read(&mut i)?; let mut y = read(&mut i)?; if relative { x1 += current.0; y1 += current.1; x2 += current.0; y2 += current.1; x += current.0; y += current.1; } path.cubic_to(x1, y1, x2, y2, x, y); current = (x, y); }
            _ => return Err(SvgError::Parse(format!("path command {command} is unsupported"))),
        }
    }
    Ok(path)
}

fn tokenize_path(data: &str) -> Result<Vec<String>> {
    let mut out = Vec::new(); let mut token = String::new();
    for ch in data.chars() {
        if ch.is_ascii_alphabetic() { if !token.is_empty() { out.push(std::mem::take(&mut token)); } out.push(ch.to_string()); }
        else if ch.is_ascii_whitespace() || ch == ',' { if !token.is_empty() { out.push(std::mem::take(&mut token)); } }
        else if ch == '-' && !token.is_empty() && !token.ends_with('e') && !token.ends_with('E') { out.push(std::mem::take(&mut token)); token.push(ch); }
        else { token.push(ch); }
        if out.len() > MAX_PATH_COMMANDS.saturating_mul(7) { return Err(SvgError::PathTooLarge); }
    }
    if !token.is_empty() { out.push(token); }
    Ok(out)
}

fn parse_numbers(value: &str) -> Result<Vec<f32>> { value.split(|c: char| c.is_ascii_whitespace() || c == ',').filter(|s| !s.is_empty()).map(|s| { let n = s.parse::<f32>().map_err(|_| SvgError::Parse("invalid numeric attribute".into()))?; if n.is_finite() { Ok(n) } else { Err(SvgError::Parse("non-finite numeric attribute".into())) } }).collect() }
fn attr_num(node: Node<'_, '_>, key: &str, default: f32) -> Result<f32> { match node.attribute(key) { None => Ok(default), Some(s) => { let n = parse_length(s).ok_or_else(|| SvgError::Parse(format!("invalid {key}")))?; if n.is_finite() { Ok(n) } else { Err(SvgError::Parse(format!("non-finite {key}"))) } } } }
fn parse_length(value: &str) -> Option<f32> { value.trim().trim_end_matches("px").parse::<f32>().ok().filter(|v| v.is_finite()) }
fn parse_viewbox(value: &str) -> Option<(f32, f32, f32, f32)> { let n = parse_numbers(value).ok()?; Some((*n.first()?, *n.get(1)?, *n.get(2)?, *n.get(3)?)) }
fn parse_color(value: &str) -> Result<Option<Color>> {
    let value = value.trim().to_ascii_lowercase();
    if value == "none" { return Ok(None); }
    let c = match value.as_str() { "black" => Color(0,0,0,255), "white" => Color(255,255,255,255), "red" => Color(255,0,0,255), "green" => Color(0,128,0,255), "blue" => Color(0,0,255,255), "yellow" => Color(255,255,0,255), "transparent" => Color(0,0,0,0), _ if value.starts_with('#') => { let hex = &value[1..]; if hex.len() == 3 { let mut n = [0u8; 3]; for (i, c) in hex.chars().enumerate() { let d = c.to_digit(16).ok_or_else(|| SvgError::Parse("invalid colour".into()))? as u8; if let Some(v) = n.get_mut(i) { *v = d.saturating_mul(17); } } Color(n[0], n[1], n[2], 255) } else if hex.len() == 6 { let n = u32::from_str_radix(hex, 16).map_err(|_| SvgError::Parse("invalid colour".into()))?; Color((n >> 16) as u8, (n >> 8) as u8, n as u8, 255) } else { return Err(SvgError::Parse("unsupported colour syntax".into())); } }, _ => return Err(SvgError::Parse(format!("unsupported colour {value}"))) };
    Ok(Some(c))
}
fn composite(rgba: &mut [u8], coverage: &[f32], color: Color, opacity: f32) {
    for (pixel, cov) in rgba.chunks_exact_mut(4).zip(coverage) {
        let alpha = (f32::from(color.3) / 255.0 * opacity * cov.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        let old = f32::from(pixel[3]) / 255.0; let out_a = alpha + old * (1.0 - alpha);
        if out_a <= 0.0 { continue; }
        for (idx, src) in [color.0, color.1, color.2].into_iter().enumerate() { let dst = f32::from(pixel[idx]); pixel[idx] = ((f32::from(src) * alpha + dst * old * (1.0 - alpha)) / out_a).round().clamp(0.0, 255.0) as u8; }
        pixel[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}
fn color_text(color: Option<Color>) -> String { match color { None => "none".into(), Some(Color(r,g,b,_)) => format!("#{r:02x}{g:02x}{b:02x}") } }
fn number(n: f32) -> Result<String> { if !n.is_finite() { return Err(SvgError::Parse("non-finite number".into())); } Ok(n.to_string()) }
fn escape_attr(value: &str) -> String { value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;") }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_document_round_trips() -> Result<()> {
        let source = "<svg width='16' height='12'><rect x='2' y='3' width='5' height='4' fill='#f00'/></svg>";
        let (doc, warnings) = SvgDocument::parse(source)?;
        assert!(warnings.is_empty());
        let (again, _) = SvgDocument::parse(&doc.to_svg()?)?;
        assert_eq!(doc, again);
        Ok(())
    }

    #[test]
    fn rasterizes_rectangle_pixels() -> Result<()> {
        let doc = SvgDocument { width: 8, height: 8, shapes: vec![Shape { kind: ShapeKind::Rect { x: 1.0, y: 1.0, width: 4.0, height: 4.0 }, fill: Some(Color(255,0,0,255)), stroke: None, stroke_width: 0.0, opacity: 1.0 }] };
        let pixels = doc.rasterize()?;
        assert_eq!(pixels.get((2 * 8 + 2) * 4), Some(&255));
        assert_eq!(pixels.get((2 * 8 + 2) * 4 + 3), Some(&255));
        Ok(())
    }

    #[test]
    fn reports_unsupported_elements() -> Result<()> {
        let (_, warnings) = SvgDocument::parse("<svg width='2' height='2'><text>title</text></svg>")?;
        assert_eq!(warnings.len(), 1);
        Ok(())
    }
}
