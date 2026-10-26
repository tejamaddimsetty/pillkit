//! Draws the widget pill to a bitmap. One renderer for every OS keeps the
//! widget pixel-identical everywhere; platform code only has to blit it.

use base64::Engine;
use fontdue::{Font, FontSettings};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Transform};

use crate::model::{Icon, Item};

pub struct Fonts {
    pub regular: Font,
    pub bold: Font,
}

impl Fonts {
    pub fn from_bytes(regular: &[u8], bold: &[u8]) -> Option<Fonts> {
        let s = FontSettings::default();
        Some(Fonts {
            regular: Font::from_bytes(regular, s).ok()?,
            bold: Font::from_bytes(bold, s).ok()?,
        })
    }

    /// Loads a sans-serif pair that ships with the OS, so we bundle no fonts.
    pub fn system() -> Option<Fonts> {
        const CANDIDATES: &[(&str, &str)] = &[
            (
                "C:\\Windows\\Fonts\\segoeui.ttf",
                "C:\\Windows\\Fonts\\segoeuib.ttf",
            ),
            (
                "/System/Library/Fonts/Supplemental/Arial.ttf",
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            ),
            (
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ),
            (
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
            ),
            (
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            ),
        ];
        CANDIDATES.iter().find_map(|(r, b)| {
            let r = std::fs::read(r).ok()?;
            let b = std::fs::read(b).ok()?;
            Fonts::from_bytes(&r, &b)
        })
    }
}

/// How big the bar is, in physical pixels.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// Height of the pill itself.
    pub height: u32,
    /// Physical pixels per logical point (1.0, 1.25, 2.0, ...).
    pub scale: f32,
    /// Widest the pill may be, in logical points.
    pub max_width_pt: f32,
    /// Every item is drawn at exactly this width, so the bar doesn't jump
    /// when items rotate and sponsored and status items look the same.
    pub fixed_width_pt: Option<f32>,
}

/// The slot size every platform uses: same width for every item.
/// Matches Sponsorbar's pill width. Wider slots are a later decision.
pub const SLOT_WIDTH_PT: f32 = 224.0;

/// Widths to step down through, widest first, when the OS has no room for the
/// full slot (macOS hides status items that don't fit beside the notch).
pub const SLOT_WIDTHS_PT: [f32; 4] = [SLOT_WIDTH_PT, 200.0, 180.0, 160.0];

/// `PILLKIT_SLOT_WIDTH=280` pins the width, for trying sizes while designing.
pub fn slot_width_override() -> Option<f32> {
    std::env::var("PILLKIT_SLOT_WIDTH")
        .ok()?
        .parse::<f32>()
        .ok()
        .map(|w| w.clamp(120.0, 480.0))
}

pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub pixmap: Pixmap,
}

impl Rendered {
    /// Premultiplied BGRA, top-down: what Windows `UpdateLayeredWindow` wants.
    pub fn to_bgra_premultiplied(&self) -> Vec<u8> {
        let mut out = self.pixmap.data().to_vec();
        for px in out.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        out
    }

    /// Straight (non-premultiplied) BGRA, top-down: what Windows icons want.
    pub fn to_bgra_straight(&self) -> Vec<u8> {
        let mut out = self.pixmap.data().to_vec();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
            px.swap(0, 2);
        }
        out
    }

    pub fn to_png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }
}

pub fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn render(item: &Item, fonts: &Fonts, m: Metrics) -> Rendered {
    let h = m.height.max(12) as f32;
    let h_pt = h / m.scale;
    let bg = parse_hex(&item.theme.background).unwrap_or([0xE8, 0xF0, 0x4A]);
    let fg = parse_hex(&item.theme.foreground).unwrap_or([0x2A, 0x2C, 0x05]);

    let pad = (h * 0.30).round();
    let icon = (h * 0.62).round();
    let gap = (h * 0.26).round();
    let max_w = (m.fixed_width_pt.unwrap_or(m.max_width_pt) * m.scale).round();

    // Tight spaces: below ~1.8x the height there's room for the icon only.
    if max_w < h * 1.8 {
        let width = max_w.max(h) as u32;
        let mut pixmap = Pixmap::new(width, h as u32).expect("non-zero size");
        rounded_rect(
            &mut pixmap,
            0.0,
            0.0,
            width as f32,
            h,
            h * 0.24,
            rgba(bg, 255),
        );
        let icon = (h * 0.66).round();
        draw_icon(
            &mut pixmap,
            item,
            fonts,
            ((width as f32 - icon) / 2.0).round(),
            ((h - icon) / 2.0).round(),
            icon,
            bg,
            fg,
        );
        return Rendered {
            width,
            height: h as u32,
            pixmap,
        };
    }
    // Compact pill (under 140 pt): one line, title only, no label chip. The
    // label still shows in the card on hover.
    let compact = m.fixed_width_pt.is_some_and(|w| w < 140.0);
    let two_line = !compact && !item.subtitle.is_empty() && h_pt >= 21.0;

    // Sizes in points, so text stays readable from a 22 pt to a 32 pt bar.
    let (title_px, sub_px) = if two_line {
        (
            (h_pt * 0.44).clamp(9.5, 14.0) * m.scale,
            (h_pt * 0.35).clamp(8.0, 11.5) * m.scale,
        )
    } else {
        let px = (h_pt * 0.5).clamp(9.5, 13.5) * m.scale;
        (px, px)
    };
    let label_px = (h_pt * 0.27).clamp(7.5, 9.5) * m.scale;
    let label_w = if compact || item.label.is_empty() {
        0.0
    } else {
        measure(&fonts.bold, label_px, &item.label) + label_px * 1.2
    };

    let text_budget =
        (max_w - pad * 2.0 - icon - gap - if label_w > 0.0 { label_w + gap } else { 0.0 }).max(h);
    let (title, subtitle, text_w) = if two_line {
        let t = fit(&fonts.bold, title_px, &item.title, text_budget);
        let s = fit(&fonts.regular, sub_px, &item.subtitle, text_budget);
        let w = measure(&fonts.bold, title_px, &t).max(measure(&fonts.regular, sub_px, &s));
        (t, s, w)
    } else {
        let joined = if compact || item.subtitle.is_empty() {
            item.title.clone()
        } else {
            format!("{} · {}", item.title, item.subtitle)
        };
        let t = fit(&fonts.bold, title_px, &joined, text_budget);
        let w = measure(&fonts.bold, title_px, &t);
        (t, String::new(), w)
    };

    let natural =
        (pad + icon + gap + text_w + if label_w > 0.0 { gap + label_w } else { 0.0 } + pad).ceil();
    let width = if m.fixed_width_pt.is_some() {
        max_w
    } else {
        natural.min(max_w)
    } as u32;
    let height = h as u32;
    let mut pixmap = Pixmap::new(width.max(1), height).expect("non-zero size");

    rounded_rect(
        &mut pixmap,
        0.0,
        0.0,
        width as f32,
        h,
        h * 0.24,
        rgba(bg, 255),
    );

    let icon_x = pad;
    let icon_y = ((h - icon) / 2.0).round();
    draw_icon(&mut pixmap, item, fonts, icon_x, icon_y, icon, bg, fg);

    let text_x = icon_x + icon + gap;
    if two_line {
        let block = title_px * 1.05 + sub_px * 1.05;
        let top = (h - block) / 2.0;
        draw_text(
            &mut pixmap,
            &fonts.bold,
            title_px,
            &title,
            text_x,
            top + title_px * 0.9,
            fg,
            255,
        );
        draw_text(
            &mut pixmap,
            &fonts.regular,
            sub_px,
            &subtitle,
            text_x,
            top + title_px * 1.05 + sub_px * 0.9,
            fg,
            215,
        );
    } else {
        draw_text(
            &mut pixmap,
            &fonts.bold,
            title_px,
            &title,
            text_x,
            h / 2.0 + title_px * 0.35,
            fg,
            255,
        );
    }

    if label_w > 0.0 {
        let lx = width as f32 - pad - label_w;
        let lh = label_px * 1.6;
        let ly = (h - lh) / 2.0;
        rounded_rect(&mut pixmap, lx, ly, label_w, lh, lh * 0.3, rgba(fg, 36));
        draw_text(
            &mut pixmap,
            &fonts.bold,
            label_px,
            &item.label,
            lx + label_px * 0.6,
            ly + lh / 2.0 + label_px * 0.36,
            fg,
            230,
        );
    }

    Rendered {
        width: pixmap.width(),
        height,
        pixmap,
    }
}

fn rgba(c: [u8; 3], a: u8) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], a)
}

fn rounded_rect(p: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        p.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_icon(
    p: &mut Pixmap,
    item: &Item,
    fonts: &Fonts,
    x: f32,
    y: f32,
    size: f32,
    bg: [u8; 3],
    fg: [u8; 3],
) {
    if let Icon::Png { data } = &item.icon {
        let decoded = base64::engine::general_purpose::STANDARD.decode(data).ok();
        if let Some(img) = decoded.and_then(|b| Pixmap::decode_png(&b).ok()) {
            let sx = size / img.width() as f32;
            let sy = size / img.height() as f32;
            let paint = PixmapPaint {
                quality: tiny_skia::FilterQuality::Bicubic,
                ..PixmapPaint::default()
            };
            p.draw_pixmap(
                0,
                0,
                img.as_ref(),
                &paint,
                Transform::from_row(sx, 0.0, 0.0, sy, x, y),
                None,
            );
            return;
        }
    }
    // Monogram: first letter of the title, background colour on a foreground disc.
    if let Some(c) = PathBuilder::from_circle(x + size / 2.0, y + size / 2.0, size / 2.0) {
        let mut paint = Paint::default();
        paint.set_color(rgba(fg, 255));
        paint.anti_alias = true;
        p.fill_path(&c, &paint, FillRule::Winding, Transform::identity(), None);
    }
    let letter: String = item
        .title
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let px = size * 0.56;
    let w = measure(&fonts.bold, px, &letter);
    draw_text(
        p,
        &fonts.bold,
        px,
        &letter,
        x + (size - w) / 2.0,
        y + size / 2.0 + px * 0.36,
        bg,
        255,
    );
}

fn measure(font: &Font, px: f32, text: &str) -> f32 {
    text.chars()
        .map(|c| font.metrics(c, px).advance_width)
        .sum()
}

fn fit(font: &Font, px: f32, text: &str, max: f32) -> String {
    if measure(font, px, text) <= max {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if measure(font, px, &candidate) <= max {
            return candidate;
        }
    }
    "…".into()
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    p: &mut Pixmap,
    font: &Font,
    px: f32,
    text: &str,
    x: f32,
    baseline: f32,
    color: [u8; 3],
    alpha: u8,
) {
    let (pw, ph) = (p.width() as i32, p.height() as i32);
    let data = p.data_mut();
    let mut pen = x;
    for ch in text.chars() {
        let (m, bitmap) = font.rasterize(ch, px);
        let gx = (pen + m.xmin as f32).round() as i32;
        let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
        for row in 0..m.height as i32 {
            for col in 0..m.width as i32 {
                let (dx, dy) = (gx + col, gy + row);
                if dx < 0 || dy < 0 || dx >= pw || dy >= ph {
                    continue;
                }
                let cov = bitmap[(row * m.width as i32 + col) as usize] as u32 * alpha as u32 / 255;
                if cov == 0 {
                    continue;
                }
                let i = ((dy * pw + dx) * 4) as usize;
                let inv = 255 - cov;
                for (k, &c) in color.iter().enumerate() {
                    data[i + k] = ((c as u32 * cov + data[i + k] as u32 * inv) / 255) as u8;
                }
                data[i + 3] = (cov + data[i + 3] as u32 * inv / 255) as u8;
            }
        }
        pen += m.advance_width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> Item {
        serde_json::from_value(serde_json::json!({
            "id": "itm_1", "title": "Acme Cloud", "subtitle": "Deploy in 30s. Try free.", "label": "Ad"
        }))
        .unwrap()
    }

    #[test]
    fn renders_within_limits() {
        let Some(fonts) = Fonts::system() else { return };
        let r = render(
            &item(),
            &fonts,
            Metrics {
                height: 48,
                scale: 2.0,
                max_width_pt: 240.0,
                fixed_width_pt: None,
            },
        );
        assert_eq!(r.height, 48);
        assert!(r.width > 48 && r.width <= 480, "width {}", r.width);
        assert!(!r.to_png().is_empty());
    }

    #[test]
    fn fixed_width_is_the_same_for_every_item() {
        let Some(fonts) = Fonts::system() else { return };
        let m = Metrics {
            height: 60,
            scale: 2.0,
            max_width_pt: 240.0,
            fixed_width_pt: Some(SLOT_WIDTH_PT),
        };
        let mut short = item();
        short.title = "Hi".into();
        short.subtitle = String::new();
        short.label = String::new();
        assert_eq!(
            render(&item(), &fonts, m).width,
            render(&short, &fonts, m).width
        );
        assert_eq!(
            render(&short, &fonts, m).width,
            (SLOT_WIDTH_PT * 2.0) as u32
        );
    }

    #[test]
    fn narrow_slots_become_compact_then_icon_only() {
        let Some(fonts) = Fonts::system() else { return };
        let m = |w: f32| Metrics {
            height: 58,
            scale: 2.0,
            max_width_pt: 480.0,
            fixed_width_pt: Some(w),
        };
        assert_eq!(render(&item(), &fonts, m(120.0)).width, 240);
        let icon = render(&item(), &fonts, m(29.0));
        assert_eq!((icon.width, icon.height), (58, 58));
        assert_eq!(icon.to_bgra_straight().len(), 58 * 58 * 4);
    }

    #[test]
    fn long_text_is_clipped() {
        let Some(fonts) = Fonts::system() else { return };
        let mut it = item();
        it.title = "A very long sponsor name that keeps going and going".into();
        let r = render(
            &it,
            &fonts,
            Metrics {
                height: 40,
                scale: 1.0,
                max_width_pt: 200.0,
                fixed_width_pt: None,
            },
        );
        assert!(r.width <= 200);
    }
}
