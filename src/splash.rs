use crate::design::{self, Icon, Palette};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};
use std::time::Instant;

const TOTAL: f32 = 6.2;
const DESK: f32 = 0.7;
const JOIN_START: f32 = 0.4;
const JOIN_STEP: f32 = 0.4;
const JOIN_TIME: f32 = 0.6;
const CURSOR_START: f32 = 2.4;
const CURSOR_END: f32 = 4.6;
const BRAND: f32 = 4.6;
const FADE_OUT: f32 = 5.6;
const NAME: &str = "mause";
const MONITORS: usize = 4;

pub struct Splash {
    start: Instant,
    done: bool,
}

impl Splash {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            done: false,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, logo: Option<egui::TextureId>) -> bool {
        let t = self.start.elapsed().as_secs_f32();
        let skip = ctx.input(|i| {
            i.pointer.any_click()
                || i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
        });
        if self.done || t >= TOTAL || skip {
            self.done = true;
            return false;
        }
        ctx.request_repaint();
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("mause_splash"))
            .order(egui::Order::Tooltip)
            .fixed_pos(screen.min)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click());
                let p = Palette::of(ui);
                let fade = 1.0 - ease(span(t, FADE_OUT, TOTAL));
                draw(ui.painter(), rect, t, fade, &p, logo);
            });
        true
    }
}

fn span(t: f32, a: f32, b: f32) -> f32 {
    ((t - a) / (b - a)).clamp(0.0, 1.0)
}

fn ease(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

fn ease_in_out(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

fn cursor_at(progress: f32) -> (usize, f32) {
    let x = progress.clamp(0.0, 1.0) * MONITORS as f32;
    let i = (x.floor() as usize).min(MONITORS - 1);
    (i, x - i as f32)
}

fn draw(
    painter: &egui::Painter,
    rect: Rect,
    t: f32,
    alpha: f32,
    p: &Palette,
    logo: Option<egui::TextureId>,
) {
    painter.rect_filled(rect, 0, fade(p.ground, alpha));
    let c = rect.center();
    let s = (rect.width() / 900.0)
        .min(rect.height() / 640.0)
        .clamp(0.55, 1.15);

    let gap = 40.0 * s;
    let row = DEVICES.iter().map(|d| d.width() * s).sum::<f32>() + gap * (MONITORS - 1) as f32;
    let baseline = c.y - 30.0 * s;
    let mut x = c.x - row / 2.0;
    let devices: Vec<Placed> = DEVICES
        .iter()
        .map(|d| {
            let placed = d.place(x, baseline, s);
            x += d.width() * s + gap;
            placed
        })
        .collect();

    let bus_y = baseline + 34.0 * s;
    let bus_left = devices[0].foot.x;
    let bus_right = devices[MONITORS - 1].foot.x;
    let joined: Vec<f32> = (0..MONITORS)
        .map(|i| {
            let start = JOIN_START + i as f32 * JOIN_STEP;
            ease(span(t, start, start + JOIN_TIME))
        })
        .collect();
    let reach = joined
        .iter()
        .enumerate()
        .filter(|(_, j)| **j > 0.0)
        .map(|(i, j)| bus_left + (devices[i].foot.x - bus_left) * j.min(1.0))
        .fold(bus_left, f32::max);
    if joined[0] > 0.0 {
        painter.line_segment(
            [pos2(bus_left, bus_y), pos2(reach.min(bus_right), bus_y)],
            Stroke::new(2.5 * s, fade(p.rule_strong, alpha)),
        );
    }

    let travel = span(t, CURSOR_START, CURSOR_END);
    let moving = t >= CURSOR_START && t < CURSOR_END + 0.4;
    let (active, along) = cursor_at(ease_in_out(travel));

    for (i, (kind, placed)) in DEVICES.iter().zip(&devices).enumerate() {
        let j = joined[i];
        if j <= 0.0 {
            continue;
        }
        let lift = vec2(0.0, (1.0 - j) * 16.0 * s);
        let placed = placed.translate(lift);
        let lit = if moving && i == active { 1.0 } else { 0.0 };
        device(painter, *kind, &placed, s, alpha * j, lit, p);
        let foot = placed.foot;
        let node = pos2(foot.x, bus_y);
        let link = span(
            t,
            JOIN_START + i as f32 * JOIN_STEP + 0.15,
            JOIN_START + i as f32 * JOIN_STEP + JOIN_TIME,
        );
        if link > 0.0 {
            let end = foot + (node - foot) * link;
            painter.line_segment(
                [foot, end],
                Stroke::new(2.5 * s, fade(p.rule_strong, alpha)),
            );
        }
        if link >= 1.0 {
            let pop = span(
                t,
                JOIN_START + i as f32 * JOIN_STEP + JOIN_TIME,
                JOIN_START + i as f32 * JOIN_STEP + JOIN_TIME + 0.35,
            );
            painter.circle_filled(node, 5.0 * s, fade(p.ok, alpha));
            if pop < 1.0 {
                painter.circle_stroke(
                    node,
                    (5.0 + 12.0 * pop) * s,
                    Stroke::new(2.0 * s, fade(p.ok, alpha * (1.0 - pop))),
                );
            }
        }
        if moving && i == active {
            let inner = placed.screen.shrink(10.0 * s);
            let y = inner.bottom() - 9.0 * s + (along * std::f32::consts::TAU).sin() * 3.0 * s;
            let at = pos2(
                inner.left() + (inner.width() - 14.0 * s) * along,
                y - 9.0 * s,
            );
            cursor(painter, at, 18.0 * s, fade(p.ground, alpha));
        }
    }

    let desk = ease(span(t, 0.0, DESK));
    let rise = (1.0 - desk) * 14.0 * s;
    let glide = if t >= CURSOR_START {
        ease_in_out(travel)
    } else {
        0.0
    };
    let desk_y = bus_y + 96.0 * s + rise;

    let font = design::wordmark(28.0 * s);
    let text_w = painter
        .layout_no_wrap(NAME.into(), font.clone(), p.ink)
        .size()
        .x;
    let (logo_s, logo_gap) = (34.0 * s, 12.0 * s);
    let brand_w = logo_s + logo_gap + text_w;
    let row_left = c.x - row / 2.0;
    let row_right = c.x + row / 2.0;
    let kb = Rect::from_min_size(pos2(row_left, desk_y - 37.0 * s), vec2(230.0, 74.0) * s);
    let mouse_size = vec2(42.0, 62.0) * s;
    let pad = Rect::from_min_max(
        pos2(
            kb.right() + 24.0 * s,
            desk_y - mouse_size.y / 2.0 - 12.0 * s,
        ),
        pos2(row_right, desk_y + mouse_size.y / 2.0 + 12.0 * s),
    );
    let (lead, tail) = (30.0 * s, 32.0 * s);
    let path = lead + brand_w + tail;
    let brand_left = pad.center().x - path / 2.0 + lead;
    let track = (brand_left - lead, brand_left + brand_w + tail);
    let mouse_x = track.0 + (track.1 - track.0) * glide;
    let mouse = Rect::from_center_size(pos2(mouse_x, desk_y), mouse_size);
    keyboard(painter, kb, s, alpha * desk, p);

    let done = ease(span(t, BRAND, BRAND + 0.5));
    let edge = if done > 0.0 {
        fade(p.accent, alpha * done)
    } else {
        fade(p.rule_strong, alpha * desk)
    };
    painter.rect(
        pad,
        14,
        fade(p.surface, alpha * desk),
        Stroke::new(1.5 * s, edge),
        StrokeKind::Inside,
    );
    let reveal = |center_x: f32| {
        if t < CURSOR_START {
            0.0
        } else {
            ((mouse_x - center_x) / (14.0 * s)).clamp(0.0, 1.0)
        }
    };
    if let Some(id) = logo {
        let shown = reveal(brand_left + logo_s / 2.0);
        if shown > 0.0 {
            painter.image(
                id,
                Rect::from_min_size(pos2(brand_left, desk_y - logo_s / 2.0), Vec2::splat(logo_s)),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                fade(Color32::WHITE, alpha * shown),
            );
        }
    }
    let text_left = brand_left + logo_s + logo_gap;
    for (i, ch) in NAME.char_indices() {
        let before = painter
            .layout_no_wrap(NAME[..i].into(), font.clone(), p.ink)
            .size()
            .x;
        let glyph = painter.layout_no_wrap(ch.to_string(), font.clone(), p.ink);
        let x = text_left + before;
        let shown = reveal(x + glyph.size().x / 2.0);
        if shown <= 0.0 {
            continue;
        }
        let galley =
            painter.layout_no_wrap(ch.to_string(), font.clone(), fade(p.ink, alpha * shown));
        let y = desk_y - galley.size().y / 2.0;
        painter.galley(pos2(x, y), galley, fade(p.ink, alpha * shown));
    }
    mouse_body(painter, mouse, s, alpha * desk, p, moving);
    for (at, phase) in [(kb.center_top(), 0.0), (mouse.center_top(), 0.5)] {
        waves(
            painter,
            at - vec2(0.0, 10.0 * s),
            s,
            alpha * desk,
            t + phase,
            p,
        );
    }
}

fn waves(painter: &egui::Painter, at: Pos2, s: f32, a: f32, t: f32, p: &Palette) {
    for k in 0..3 {
        let radius = (8.0 + k as f32 * 7.0) * s;
        let beat = ((t * 2.4 - k as f32 * 0.22).rem_euclid(1.0) * std::f32::consts::PI).sin();
        let points: Vec<Pos2> = (0..=12)
            .map(|n| {
                let angle = (-140.0 + n as f32 * 100.0 / 12.0).to_radians();
                at + vec2(angle.cos(), angle.sin()) * radius
            })
            .collect();
        painter.add(Shape::line(
            points,
            Stroke::new(2.2 * s, fade(p.accent, a * (0.25 + 0.75 * beat))),
        ));
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Device {
    Desktop,
    Laptop,
    Tablet,
    Phone,
}

const DEVICES: [Device; MONITORS] = [
    Device::Desktop,
    Device::Laptop,
    Device::Tablet,
    Device::Phone,
];

struct Placed {
    body: Rect,
    screen: Rect,
    foot: Pos2,
}

impl Placed {
    fn translate(&self, d: Vec2) -> Self {
        Self {
            body: self.body.translate(d),
            screen: self.screen.translate(d),
            foot: self.foot + d,
        }
    }
}

impl Device {
    fn width(self) -> f32 {
        match self {
            Device::Desktop => 140.0,
            Device::Laptop => 156.0,
            Device::Tablet => 74.0,
            Device::Phone => 46.0,
        }
    }

    fn os(self) -> Icon {
        match self {
            Device::Desktop => Icon::Linux,
            Device::Laptop => Icon::Windows,
            Device::Tablet => Icon::Apple,
            Device::Phone => Icon::Android,
        }
    }

    fn place(self, x: f32, baseline: f32, s: f32) -> Placed {
        let w = self.width() * s;
        let center = x + w / 2.0;
        let (body, screen) = match self {
            Device::Desktop => {
                let body = Rect::from_min_max(
                    pos2(x, baseline - 110.0 * s),
                    pos2(x + w, baseline - 20.0 * s),
                );
                (body, body.shrink(6.0 * s))
            }
            Device::Laptop => {
                let lid = Rect::from_center_size(
                    pos2(center, baseline - 8.0 * s - 42.0 * s),
                    vec2(128.0, 84.0) * s,
                );
                (lid, lid.shrink(6.0 * s))
            }
            Device::Tablet => {
                let body = Rect::from_min_max(pos2(x, baseline - 104.0 * s), pos2(x + w, baseline));
                (body, body.shrink(7.0 * s))
            }
            Device::Phone => {
                let body = Rect::from_min_max(pos2(x, baseline - 88.0 * s), pos2(x + w, baseline));
                let screen = Rect::from_min_max(
                    body.min + vec2(4.0, 12.0) * s,
                    body.max - vec2(4.0, 6.0) * s,
                );
                (body, screen)
            }
        };
        Placed {
            body,
            screen,
            foot: pos2(center, baseline),
        }
    }
}

fn device(
    painter: &egui::Painter,
    kind: Device,
    d: &Placed,
    s: f32,
    a: f32,
    lit: f32,
    p: &Palette,
) {
    let frame = fade(p.ink2, a);
    let r = d.body;
    let corner = match kind {
        Device::Desktop | Device::Laptop => 10,
        Device::Tablet => 12,
        Device::Phone => 10,
    };
    match kind {
        Device::Desktop => {
            let neck = Rect::from_min_size(
                pos2(r.center().x - 5.0 * s, r.bottom()),
                vec2(10.0, 14.0) * s,
            );
            painter.rect_filled(neck, 2, frame);
            let base =
                Rect::from_center_size(pos2(r.center().x, d.foot.y - 3.0 * s), vec2(46.0, 6.0) * s);
            painter.rect_filled(base, 3, frame);
        }
        Device::Laptop => {
            let deck = Rect::from_min_max(
                pos2(d.foot.x - 78.0 * s, r.bottom() + 1.0 * s),
                pos2(d.foot.x + 78.0 * s, d.foot.y),
            );
            painter.rect_filled(deck, 4, frame);
            let notch =
                Rect::from_center_size(pos2(d.foot.x, deck.top() + 2.0 * s), vec2(24.0, 3.0) * s);
            painter.rect_filled(notch, 1, fade(p.ground, a));
        }
        Device::Tablet | Device::Phone => {}
    }
    painter.rect_filled(r, corner, fade(p.surface, a));
    let screen_corner = if kind == Device::Phone { 6 } else { 5 };
    painter.rect_filled(d.screen, screen_corner, fade(p.inset, a));
    if lit > 0.0 {
        painter.rect_filled(d.screen, screen_corner, fade(p.accent, a * lit));
    }
    painter.rect_stroke(r, corner, Stroke::new(2.5 * s, frame), StrokeKind::Inside);
    if kind == Device::Phone {
        let pill =
            Rect::from_center_size(pos2(r.center().x, r.top() + 7.0 * s), vec2(14.0, 4.0) * s);
        painter.rect_filled(pill, 2, frame);
    }
    let logo_size = (d.screen.height().min(d.screen.width()) * 0.58).min(42.0 * s);
    let ink = if lit > 0.0 {
        fade(p.ground, a * 0.85)
    } else {
        fade(p.ink2, a)
    };
    let at = d.screen.center() - vec2(0.0, d.screen.height() * 0.1);
    design::icon(painter, at, kind.os(), ink, logo_size);
}

fn keyboard(painter: &egui::Painter, r: Rect, s: f32, a: f32, p: &Palette) {
    painter.rect(
        r,
        12,
        fade(p.surface, a),
        Stroke::new(2.0 * s, fade(p.ink2, a)),
        StrokeKind::Inside,
    );
    let inner = r.shrink(12.0 * s);
    let (rows, cols) = (3, 10);
    let key = vec2(inner.width() / cols as f32, inner.height() / rows as f32);
    for row in 0..rows {
        for col in 0..cols {
            let k = Rect::from_min_size(
                inner.min + vec2(col as f32 * key.x, row as f32 * key.y),
                key,
            )
            .shrink(2.0 * s);
            painter.rect_filled(k, 3, fade(p.inset, a));
        }
    }
    let space = Rect::from_center_size(
        pos2(inner.center().x, inner.bottom() - key.y / 2.0),
        vec2(key.x * 4.0, key.y) - Vec2::splat(4.0 * s),
    );
    painter.rect_filled(space, 3, fade(p.hover, a));
}

fn mouse_body(painter: &egui::Painter, r: Rect, s: f32, a: f32, p: &Palette, moving: bool) {
    painter.rect(
        r,
        (r.width() / 2.0) as u8,
        fade(p.surface, a),
        Stroke::new(2.0 * s, fade(p.ink2, a)),
        StrokeKind::Inside,
    );
    let wheel = Rect::from_center_size(r.center_top() + vec2(0.0, 16.0 * s), vec2(5.0, 12.0) * s);
    let color = if moving { p.accent } else { p.ink2 };
    painter.rect_filled(wheel, 3, fade(color, a));
}

fn cursor(painter: &egui::Painter, tip: Pos2, h: f32, color: Color32) {
    let notch = tip + vec2(h * 0.3, h * 0.72);
    for pts in [
        vec![tip, tip + vec2(0.0, h), notch],
        vec![tip, notch, tip + vec2(h * 0.72, h * 0.72)],
    ] {
        painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    }
    painter.line_segment(
        [tip + vec2(h * 0.28, h * 0.6), tip + vec2(h * 0.5, h * 1.08)],
        Stroke::new(h * 0.17, color),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_computer_joins_before_the_cursor_starts_and_the_brand_follows() {
        let last_join = JOIN_START + (MONITORS - 1) as f32 * JOIN_STEP + JOIN_TIME;
        assert!(last_join <= CURSOR_START, "son katılım {last_join}");
        assert!(CURSOR_START < CURSOR_END && CURSOR_END <= FADE_OUT);
        assert!(BRAND < FADE_OUT && FADE_OUT < TOTAL);
    }

    #[test]
    fn the_cursor_crosses_every_screen_from_left_to_right() {
        assert_eq!(cursor_at(0.0), (0, 0.0));
        let (i, u) = cursor_at(0.5);
        assert_eq!((i, u), (2, 0.0));
        assert_eq!(cursor_at(1.0).0, MONITORS - 1);
        let mut last = (0, 0.0);
        for n in 1..=100 {
            let now = cursor_at(n as f32 / 100.0);
            assert!(now.0 > last.0 || (now.0 == last.0 && now.1 >= last.1));
            last = now;
        }
    }
}
