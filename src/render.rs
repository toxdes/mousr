use std::{
    collections::{BTreeSet, HashMap},
    fs,
};

use fontdue::{Font, FontSettings, Metrics};
use thiserror::Error;
use tiny_skia::{Color as SkColor, Paint, PathBuilder, Pixmap, Rect as SkRect, Stroke, Transform};

use crate::{
    config::{Color, Ui, Unmatched},
    font_data,
    grid::{Layout, Rect},
};

pub struct ActionHint {
    pub key: String,
    pub action: &'static str,
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("embedded fallback font is invalid: {0}")]
    InvalidFallback(&'static str),
    #[error("cannot allocate a {width}x{height} render buffer")]
    Allocation { width: u32, height: u32 },
}

struct Glyph {
    metrics: Metrics,
    alpha: Vec<u8>,
}

pub struct Renderer {
    font: Font,
    glyphs: HashMap<(char, u32), Glyph>,
}

impl Renderer {
    pub fn new(ui: &Ui) -> Result<(Self, Option<String>), RenderError> {
        let mut warning = None;
        let font = ui.font_path.as_ref().and_then(|path| match fs::read(path) {
            Ok(bytes) => match Font::from_bytes(bytes, FontSettings::default()) {
                Ok(font) => Some(font),
                Err(error) => {
                    warning = Some(format!(
                        "cannot parse font {}: {error}; using embedded fallback",
                        path.display()
                    ));
                    None
                }
            },
            Err(error) => {
                warning = Some(format!(
                    "cannot read font {}: {error}; using embedded fallback",
                    path.display()
                ));
                None
            }
        });
        let font = match font {
            Some(font) => font,
            None => Font::from_bytes(font_data::fallback_font(), FontSettings::default())
                .map_err(RenderError::InvalidFallback)?,
        };
        Ok((
            Self {
                font,
                glyphs: HashMap::new(),
            },
            warning,
        ))
    }

    pub fn render_grid(&mut self, request: GridRender<'_>) -> Result<Frame, RenderError> {
        let mut pixmap =
            Pixmap::new(request.width, request.height).ok_or(RenderError::Allocation {
                width: request.width,
                height: request.height,
            })?;
        pixmap.fill(sk_color(request.ui.overlay_background, 1.0));

        let mut vertical = BTreeSet::new();
        let mut horizontal = BTreeSet::new();
        for (index, tile) in request.layout.tiles.iter().enumerate() {
            if tile.output != request.output {
                continue;
            }
            let matches = tile.label.starts_with(request.prefix);
            let visibility = match (matches, request.unmatched) {
                (true, _) | (false, Unmatched::Keep) => 1.0,
                (false, Unmatched::Dim) => request.unmatched_opacity,
                (false, Unmatched::Hide) => 0.0,
            };
            if visibility > 0.0 {
                fill_rect(
                    &mut pixmap,
                    tile.bounds,
                    request.ui.cell_background,
                    visibility,
                );
                self.draw_label(
                    &mut pixmap,
                    LabelRender {
                        bounds: tile.bounds,
                        label: &tile.label,
                        prefix: request.prefix,
                        matches,
                        opacity: visibility,
                        ui: request.ui,
                    },
                );
            }
            if visibility > 0.0 {
                vertical.insert(tile.bounds.x);
                vertical.insert(tile.bounds.x + tile.bounds.width);
                horizontal.insert(tile.bounds.y);
                horizontal.insert(tile.bounds.y + tile.bounds.height);
            }
            if request.selected == Some(index) {
                fill_rect(
                    &mut pixmap,
                    tile.bounds,
                    request.ui.selected_background,
                    1.0,
                );
                stroke_rect(
                    &mut pixmap,
                    tile.bounds,
                    request.ui.selected_border,
                    request.ui.selected_border_width,
                );
            }
        }
        draw_grid_lines(
            &mut pixmap,
            &vertical,
            &horizontal,
            request.ui.grid_border,
            request.ui.grid_border_width,
        );
        if let Some(selected) = request
            .selected
            .and_then(|index| request.layout.tiles.get(index))
            && selected.output == request.output
            && !request.hints.is_empty()
        {
            let area = largest_free_area(request.width, request.height, selected.bounds);
            self.draw_action_hints(&mut pixmap, request.hints, area, request.ui, true);
        }
        Ok(Frame::from_pixmap(pixmap))
    }

    pub fn render_badge(&mut self, text: &str, ui: &Ui) -> Result<Frame, RenderError> {
        let scale = font_scale(ui.font_size);
        let width = text
            .chars()
            .try_fold(24.0_f32, |width, character| {
                let glyph = self.glyph(character, scale);
                Some(width + glyph.metrics.advance_width)
            })
            .unwrap_or(240.0)
            .ceil() as u32;
        let height = (ui.font_size * 1.8).ceil() as u32;
        let mut pixmap = Pixmap::new(width.max(1), height.max(1))
            .ok_or(RenderError::Allocation { width, height })?;
        pixmap.fill(sk_color(ui.badge_background, 1.0));
        stroke_rect(
            &mut pixmap,
            Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            ui.badge_border,
            ui.badge_border_width,
        );
        self.draw_text(
            &mut pixmap,
            text,
            (12.0, (f64::from(height) * 0.68) as f32),
            TextStyle {
                size: ui.font_size,
                color: ui.badge_foreground,
                opacity: 1.0,
            },
        );
        Ok(Frame::from_pixmap(pixmap))
    }

    pub fn render_ring(&self, ui: &Ui) -> Result<Frame, RenderError> {
        let padding = ui.target_ring_width.ceil() as u32 + 2;
        let size = (ui.target_ring_radius.ceil() as u32 + padding) * 2;
        let mut pixmap = Pixmap::new(size, size).ok_or(RenderError::Allocation {
            width: size,
            height: size,
        })?;
        let mut path = PathBuilder::new();
        path.push_circle(size as f32 / 2.0, size as f32 / 2.0, ui.target_ring_radius);
        let mut paint = Paint::default();
        paint.set_color(sk_color(ui.target_ring, 1.0));
        if let Some(path) = path.finish() {
            pixmap.stroke_path(
                &path,
                &paint,
                &Stroke {
                    width: ui.target_ring_width,
                    ..Stroke::default()
                },
                Transform::identity(),
                None,
            );
        }
        Ok(Frame::from_pixmap(pixmap))
    }

    pub fn render_mode(
        &mut self,
        width: u32,
        height: u32,
        badge: &str,
        target: Option<(u32, u32)>,
        hints: &[ActionHint],
        ui: &Ui,
    ) -> Result<Frame, RenderError> {
        let mut pixmap =
            Pixmap::new(width, height).ok_or(RenderError::Allocation { width, height })?;
        if ui.show_badge {
            let scale = font_scale(ui.font_size);
            let text_width: f32 = badge
                .chars()
                .map(|character| self.glyph(character, scale).metrics.advance_width)
                .sum();
            let badge_width = (text_width + 24.0).ceil();
            let badge_height = (ui.font_size * 1.8).ceil();
            fill_sk_rect(
                &mut pixmap,
                12.0,
                12.0,
                badge_width,
                badge_height,
                ui.badge_background,
                1.0,
            );
            stroke_rect(
                &mut pixmap,
                Rect {
                    x: 12,
                    y: 12,
                    width: badge_width as u32,
                    height: badge_height as u32,
                },
                ui.badge_border,
                ui.badge_border_width,
            );
            self.draw_text(
                &mut pixmap,
                badge,
                (24.0, 12.0 + badge_height * 0.68),
                TextStyle {
                    size: ui.font_size,
                    color: ui.badge_foreground,
                    opacity: 1.0,
                },
            );
        }
        if ui.show_target_ring
            && let Some((x, y)) = target
        {
            let mut path = PathBuilder::new();
            path.push_circle(x as f32, y as f32, ui.target_ring_radius);
            let mut paint = Paint::default();
            paint.set_color(sk_color(ui.target_ring, 1.0));
            if let Some(path) = path.finish() {
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &Stroke {
                        width: ui.target_ring_width,
                        ..Stroke::default()
                    },
                    Transform::identity(),
                    None,
                );
            }
        }
        if !hints.is_empty() {
            self.draw_action_hints(
                &mut pixmap,
                hints,
                Rect {
                    x: 0,
                    y: 0,
                    width,
                    height,
                },
                ui,
                false,
            );
        }
        Ok(Frame::from_pixmap(pixmap))
    }

    fn draw_action_hints(
        &mut self,
        pixmap: &mut Pixmap,
        hints: &[ActionHint],
        area: Rect,
        ui: &Ui,
        background: bool,
    ) {
        let font_size = ui.font_size.max(11.0);
        let scale = font_scale(font_size);
        let key_width = hints
            .iter()
            .map(|hint| self.measure_text(&hint.key, scale))
            .fold(0.0_f32, f32::max);
        let action_width = hints
            .iter()
            .map(|hint| self.measure_text(hint.action, scale))
            .fold(0.0_f32, f32::max);
        let padding = 14.0;
        let key_gap = 10.0;
        let column_gap = 28.0;
        let entry_width = key_width + key_gap + action_width;
        let two_column_width = padding * 2.0 + entry_width * 2.0 + column_gap;
        let columns = usize::from(hints.len() > 5 && two_column_width <= area.width as f32);
        let columns = columns + 1;
        let rows = hints.len().div_ceil(columns);
        let width = padding * 2.0
            + entry_width * columns as f32
            + column_gap * columns.saturating_sub(1) as f32;
        let row_height = font_size * 1.55;
        let height = padding * 2.0 + row_height * rows as f32;
        if width > area.width as f32 || height > area.height as f32 {
            return;
        }
        let left = if background {
            area.x as f32 + (area.width as f32 - width) / 2.0
        } else {
            area.x as f32 + 18.0
        };
        let top = if background {
            area.y as f32 + (area.height as f32 - height) / 2.0
        } else {
            area.y as f32 + area.height as f32 - height - 18.0
        };
        if background {
            fill_sk_rect(pixmap, left, top, width, height, ui.badge_background, 1.0);
            stroke_sk_rect(
                pixmap,
                left,
                top,
                width,
                height,
                ui.badge_border,
                ui.badge_border_width,
            );
        }
        for (index, hint) in hints.iter().enumerate() {
            let column = index / rows;
            let row = index % rows;
            let x = left + padding + column as f32 * (entry_width + column_gap);
            let baseline = top + padding + row_height * row as f32 + font_size;
            self.draw_hint_text(
                pixmap,
                &hint.key,
                (x, baseline),
                font_size,
                ui.matched_background,
                background,
            );
            self.draw_hint_text(
                pixmap,
                hint.action,
                (x + key_width + key_gap, baseline),
                font_size,
                ui.badge_foreground,
                background,
            );
        }
    }

    fn draw_hint_text(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        position: (f32, f32),
        size: f32,
        color: Color,
        background: bool,
    ) {
        if !background {
            for (x, y) in [
                (-1.0, -1.0),
                (0.0, -1.0),
                (1.0, -1.0),
                (-1.0, 0.0),
                (1.0, 0.0),
                (-1.0, 1.0),
                (0.0, 1.0),
                (1.0, 1.0),
            ] {
                self.draw_text(
                    pixmap,
                    text,
                    (position.0 + x, position.1 + y),
                    TextStyle {
                        size,
                        color: Color([0, 0, 0, 230]),
                        opacity: 1.0,
                    },
                );
            }
        }
        self.draw_text(
            pixmap,
            text,
            position,
            TextStyle {
                size,
                color,
                opacity: 1.0,
            },
        );
    }

    fn measure_text(&mut self, text: &str, scale: u32) -> f32 {
        text.chars()
            .map(|character| self.glyph(character, scale).metrics.advance_width)
            .sum()
    }

    fn draw_label(&mut self, pixmap: &mut Pixmap, request: LabelRender<'_>) {
        let LabelRender {
            bounds,
            label,
            prefix,
            matches,
            opacity,
            ui,
        } = request;
        let scale = font_scale(ui.font_size);
        let advances: Vec<f32> = label
            .chars()
            .map(|character| self.glyph(character, scale).metrics.advance_width)
            .collect();
        let text_width: f32 = advances.iter().sum();
        let padding = (ui.font_size * 0.65).max(6.0);
        let pill_width = text_width + padding * 2.0;
        let pill_height = ui.font_size * 1.3;
        let left = bounds.x as f32 + (bounds.width as f32 - pill_width) / 2.0;
        let top = bounds.y as f32 + (bounds.height as f32 - pill_height) / 2.0;
        fill_sk_rect(
            pixmap,
            left,
            top,
            pill_width,
            pill_height,
            ui.label_background,
            opacity,
        );

        let mut x = left + padding;
        let baseline = top + ui.font_size;
        let prefix_len = prefix.chars().count();
        for (index, (character, advance)) in label.chars().zip(advances).enumerate() {
            let matched = matches && index < prefix_len;
            if matched {
                fill_sk_rect(
                    pixmap,
                    x,
                    top,
                    advance,
                    pill_height,
                    ui.matched_background,
                    opacity,
                );
            }
            let color = if matched {
                ui.matched_foreground
            } else {
                ui.label_foreground
            };
            self.draw_character(
                pixmap,
                character,
                (x, baseline),
                TextStyle {
                    size: ui.font_size,
                    color,
                    opacity,
                },
            );
            x += advance;
        }
    }

    fn draw_text(
        &mut self,
        pixmap: &mut Pixmap,
        text: &str,
        position: (f32, f32),
        style: TextStyle,
    ) {
        let (mut x, baseline) = position;
        let scale = font_scale(style.size);
        for character in text.chars() {
            let advance = self.glyph(character, scale).metrics.advance_width;
            self.draw_character(pixmap, character, (x, baseline), style);
            x += advance;
        }
    }

    fn draw_character(
        &mut self,
        pixmap: &mut Pixmap,
        character: char,
        position: (f32, f32),
        style: TextStyle,
    ) {
        let (x, baseline) = position;
        let scale = font_scale(style.size);
        let glyph = self.glyph(character, scale);
        let left = x.floor() as i32 + glyph.metrics.xmin;
        let top = baseline.floor() as i32 - glyph.metrics.height as i32 - glyph.metrics.ymin;
        let [red, green, blue, alpha] = style.color.0;
        for row in 0..glyph.metrics.height {
            for column in 0..glyph.metrics.width {
                let coverage = glyph.alpha[row * glyph.metrics.width + column];
                if coverage == 0 {
                    continue;
                }
                let px = left + column as i32;
                let py = top + row as i32;
                if px < 0 || py < 0 || px >= pixmap.width() as i32 || py >= pixmap.height() as i32 {
                    continue;
                }
                let effective_alpha =
                    (f32::from(alpha) * style.opacity * f32::from(coverage) / 255.0).round() as u8;
                blend_pixel(
                    pixmap,
                    px as u32,
                    py as u32,
                    [red, green, blue, effective_alpha],
                );
            }
        }
    }

    fn glyph(&mut self, character: char, scale: u32) -> &Glyph {
        self.glyphs.entry((character, scale)).or_insert_with(|| {
            let (metrics, alpha) = self.font.rasterize(character, scale as f32 / 64.0);
            Glyph { metrics, alpha }
        })
    }
}

struct LabelRender<'a> {
    bounds: Rect,
    label: &'a str,
    prefix: &'a str,
    matches: bool,
    opacity: f32,
    ui: &'a Ui,
}

#[derive(Clone, Copy)]
struct TextStyle {
    size: f32,
    color: Color,
    opacity: f32,
}

pub struct GridRender<'a> {
    pub width: u32,
    pub height: u32,
    pub output: &'a str,
    pub layout: &'a Layout,
    pub prefix: &'a str,
    pub selected: Option<usize>,
    pub hints: &'a [ActionHint],
    pub unmatched: Unmatched,
    pub unmatched_opacity: f32,
    pub ui: &'a Ui,
}

fn largest_free_area(width: u32, height: u32, selected: Rect) -> Rect {
    let right = selected.x.saturating_add(selected.width).min(width);
    let bottom = selected.y.saturating_add(selected.height).min(height);
    [
        Rect {
            x: 0,
            y: 0,
            width: selected.x.min(width),
            height,
        },
        Rect {
            x: right,
            y: 0,
            width: width.saturating_sub(right),
            height,
        },
        Rect {
            x: 0,
            y: 0,
            width,
            height: selected.y.min(height),
        },
        Rect {
            x: 0,
            y: bottom,
            width,
            height: height.saturating_sub(bottom),
        },
    ]
    .into_iter()
    .max_by_key(|area| u64::from(area.width) * u64::from(area.height))
    .unwrap_or(Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    })
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub argb8888: Vec<u8>,
}

impl Frame {
    fn from_pixmap(pixmap: Pixmap) -> Self {
        let width = pixmap.width();
        let height = pixmap.height();
        let mut argb8888 = pixmap.take();
        for pixel in argb8888.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Self {
            width,
            height,
            argb8888,
        }
    }
}

fn font_scale(size: f32) -> u32 {
    (size.max(1.0) * 64.0).round() as u32
}

fn fill_rect(pixmap: &mut Pixmap, rect: Rect, color: Color, opacity: f32) {
    fill_sk_rect(
        pixmap,
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
        color,
        opacity,
    );
}

fn fill_sk_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    opacity: f32,
) {
    let Some(rect) = SkRect::from_xywh(x, y, width, height) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(sk_color(color, opacity));
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn stroke_rect(pixmap: &mut Pixmap, rect: Rect, color: Color, width: f32) {
    stroke_sk_rect(
        pixmap,
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
        color,
        width,
    );
}

fn stroke_sk_rect(
    pixmap: &mut Pixmap,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    stroke_width: f32,
) {
    let Some(sk_rect) = SkRect::from_xywh(x, y, width, height) else {
        return;
    };
    let path = PathBuilder::from_rect(sk_rect);
    let mut paint = Paint::default();
    paint.set_color(sk_color(color, 1.0));
    pixmap.stroke_path(
        &path,
        &paint,
        &Stroke {
            width: stroke_width,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
}

fn draw_grid_lines(
    pixmap: &mut Pixmap,
    vertical: &BTreeSet<u32>,
    horizontal: &BTreeSet<u32>,
    color: Color,
    width: f32,
) {
    let mut builder = PathBuilder::new();
    for x in vertical {
        builder.move_to(*x as f32, 0.0);
        builder.line_to(*x as f32, pixmap.height() as f32);
    }
    for y in horizontal {
        builder.move_to(0.0, *y as f32);
        builder.line_to(pixmap.width() as f32, *y as f32);
    }
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(sk_color(color, 1.0));
    pixmap.stroke_path(
        &path,
        &paint,
        &Stroke {
            width,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
}

fn sk_color(color: Color, opacity: f32) -> SkColor {
    let [red, green, blue, alpha] = color.0;
    SkColor::from_rgba8(
        red,
        green,
        blue,
        (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8,
    )
}

fn blend_pixel(pixmap: &mut Pixmap, x: u32, y: u32, source: [u8; 4]) {
    let index = ((y * pixmap.width() + x) * 4) as usize;
    let destination = &mut pixmap.data_mut()[index..index + 4];
    let source_alpha = u32::from(source[3]);
    let inverse = 255 - source_alpha;
    for channel in 0..3 {
        destination[channel] = ((u32::from(source[channel]) * source_alpha
            + u32::from(destination[channel]) * inverse)
            / 255) as u8;
    }
    destination[3] = (source_alpha + u32::from(destination[3]) * inverse / 255).min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{self, Region, Settings};

    fn layout() -> Layout {
        grid::build(
            &[Region {
                output: "DP-1".into(),
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: 320,
                    height: 180,
                },
            }],
            Settings {
                min_tile_width: 24,
                min_tile_height: 24,
                max_label_length: 1,
                max_cells: 8,
            },
        )
        .unwrap()
    }

    #[test]
    fn renders_a_wayland_sized_buffer() {
        let ui = Ui::default();
        let (mut renderer, warning) = Renderer::new(&ui).unwrap();
        assert!(warning.is_none());
        let frame = renderer
            .render_grid(GridRender {
                width: 320,
                height: 180,
                output: "DP-1",
                layout: &layout(),
                prefix: "",
                selected: None,
                hints: &[],
                unmatched: Unmatched::Dim,
                unmatched_opacity: 0.18,
                ui: &ui,
            })
            .unwrap();
        assert_eq!(frame.argb8888.len(), 320 * 180 * 4);
        assert!(frame.argb8888.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn hidden_unmatched_cells_render_less_content() {
        let ui = Ui::default();
        let layout = layout();
        let (mut renderer, _) = Renderer::new(&ui).unwrap();
        let all = renderer
            .render_grid(GridRender {
                width: 320,
                height: 180,
                output: "DP-1",
                layout: &layout,
                prefix: "",
                selected: None,
                hints: &[],
                unmatched: Unmatched::Hide,
                unmatched_opacity: 0.0,
                ui: &ui,
            })
            .unwrap();
        let filtered = renderer
            .render_grid(GridRender {
                width: 320,
                height: 180,
                output: "DP-1",
                layout: &layout,
                prefix: "a",
                selected: None,
                hints: &[],
                unmatched: Unmatched::Hide,
                unmatched_opacity: 0.0,
                ui: &ui,
            })
            .unwrap();
        let all_alpha: u64 = all
            .argb8888
            .chunks_exact(4)
            .map(|pixel| u64::from(pixel[3]))
            .sum();
        let filtered_alpha: u64 = filtered
            .argb8888
            .chunks_exact(4)
            .map(|pixel| u64::from(pixel[3]))
            .sum();
        assert!(filtered_alpha < all_alpha);
    }

    #[test]
    fn selected_grid_hints_change_the_free_area() {
        let ui = Ui::default();
        let layout = layout();
        let hints = [ActionHint {
            key: "f".into(),
            action: "Left click",
        }];
        let (mut renderer, _) = Renderer::new(&ui).unwrap();
        let without = renderer
            .render_grid(GridRender {
                width: 320,
                height: 180,
                output: "DP-1",
                layout: &layout,
                prefix: "a",
                selected: Some(0),
                hints: &[],
                unmatched: Unmatched::Hide,
                unmatched_opacity: 0.0,
                ui: &ui,
            })
            .unwrap();
        let with = renderer
            .render_grid(GridRender {
                width: 320,
                height: 180,
                output: "DP-1",
                layout: &layout,
                prefix: "a",
                selected: Some(0),
                hints: &hints,
                unmatched: Unmatched::Hide,
                unmatched_opacity: 0.0,
                ui: &ui,
            })
            .unwrap();
        assert_ne!(with.argb8888, without.argb8888);
    }

    #[test]
    fn mouse_hints_keep_the_overlay_background_transparent() {
        let ui = Ui {
            show_badge: false,
            show_target_ring: false,
            ..Ui::default()
        };
        let hints = [ActionHint {
            key: "h j k l".into(),
            action: "Move pointer",
        }];
        let (mut renderer, _) = Renderer::new(&ui).unwrap();
        let frame = renderer
            .render_mode(640, 360, "MOUSE", None, &hints, &ui)
            .unwrap();
        assert!(frame.argb8888.chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert_eq!(&frame.argb8888[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn free_area_does_not_overlap_selected_cell() {
        let selected = Rect {
            x: 450,
            y: 350,
            width: 100,
            height: 100,
        };
        let area = largest_free_area(1000, 800, selected);
        let separated = area.x + area.width <= selected.x
            || selected.x + selected.width <= area.x
            || area.y + area.height <= selected.y
            || selected.y + selected.height <= area.y;
        assert!(separated);
        assert_eq!(u64::from(area.width) * u64::from(area.height), 360_000);
    }
}
