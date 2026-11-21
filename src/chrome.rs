use crate::design::{self, Icon, Palette};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy)]
enum Caption {
    Minimize,
    Maximize,
    Close,
}

pub const CONTROLS_WIDTH: f32 = 138.0;

pub fn titlebar(
    root: &mut egui::Ui,
    logo: Option<egui::TextureId>,
    center: impl FnOnce(&mut egui::Ui),
) {
    let ctx = root.ctx().clone();
    let palette = Palette::new(ctx.global_style().visuals.dark_mode);
    egui::Panel::top("window_caption")
        .show_separator_line(false)
        .exact_size(crate::gui::TOPBAR)
        .resizable(false)
        .frame(egui::Frame::new().fill(palette.ground))
        .show(root, |ui| {
            let bounds = ui.max_rect();
            let controls_width = CONTROLS_WIDTH;
            let drag_rect = Rect::from_min_max(
                bounds.min,
                egui::pos2(bounds.right() - controls_width, bounds.bottom()),
            );
            let drag = ui.interact(
                drag_rect,
                ui.scope_id().with("drag_window"),
                Sense::click_and_drag(),
            );
            if drag.double_clicked() {
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            } else if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            crate::design::brand(
                ui.painter(),
                bounds.left_center() + Vec2::new(24.0, 0.0),
                logo,
                palette,
            );
            center(ui);
            for (index, action, label) in [
                (0, Caption::Minimize, "Küçült"),
                (1, Caption::Maximize, "Büyüt / geri yükle"),
                (2, Caption::Close, "Kapat"),
            ] {
                let rect = Rect::from_min_size(
                    egui::pos2(
                        bounds.right() - controls_width + index as f32 * 46.0,
                        bounds.top(),
                    ),
                    Vec2::new(46.0, 52.0),
                );
                let response = ui.put(
                    rect,
                    egui::Button::new("")
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE),
                );
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                let danger = matches!(action, Caption::Close) && response.hovered();
                if response.hovered() || response.has_focus() {
                    ui.painter().rect_filled(
                        rect,
                        0,
                        if danger { palette.bad } else { palette.hover },
                    );
                }
                let ink = if danger { palette.ground } else { palette.ink };
                let glyph = match action {
                    Caption::Minimize => Icon::Minimize,
                    Caption::Maximize if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) => {
                        Icon::Restore
                    }
                    Caption::Maximize => Icon::Maximize,
                    Caption::Close => Icon::Close,
                };
                design::icon(ui.painter(), rect.center(), glyph, ink, 16.0);
                if response.clicked() {
                    let command = match action {
                        Caption::Minimize => egui::ViewportCommand::Minimized(true),
                        Caption::Maximize => egui::ViewportCommand::Maximized(
                            !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                        ),
                        Caption::Close => egui::ViewportCommand::Close,
                    };
                    ctx.send_viewport_cmd(command);
                }
                response.on_hover_text(label);
            }
            ui.allocate_rect(bounds, Sense::hover());
        });
}

pub fn resize_edges(root: &mut egui::Ui) {
    if root
        .ctx()
        .input(|i| i.viewport().maximized.unwrap_or(false))
    {
        return;
    }
    let bounds = root.max_rect();
    let Some(pos) = root.ctx().pointer_hover_pos() else {
        return;
    };
    let edge = 5.0;
    let left = pos.x < bounds.left() + edge;
    let right = pos.x > bounds.right() - edge;
    let top = pos.y < bounds.top() + edge;
    let bottom = pos.y > bounds.bottom() - edge;
    use egui::{CursorIcon as C, ResizeDirection as R};
    let (direction, cursor) = match (left, right, top, bottom) {
        (true, _, true, _) => (R::NorthWest, C::ResizeNorthWest),
        (_, true, true, _) => (R::NorthEast, C::ResizeNorthEast),
        (true, _, _, true) => (R::SouthWest, C::ResizeSouthWest),
        (_, true, _, true) => (R::SouthEast, C::ResizeSouthEast),
        (true, _, _, _) => (R::West, C::ResizeWest),
        (_, true, _, _) => (R::East, C::ResizeEast),
        (_, _, true, _) => (R::North, C::ResizeNorth),
        (_, _, _, true) => (R::South, C::ResizeSouth),
        _ => return,
    };
    root.ctx().set_cursor_icon(cursor);
    if root.ctx().input(|i| i.pointer.primary_pressed()) {
        root.ctx()
            .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_buttons_emit_native_commands_in_both_themes() {
        for theme in [1, 2] {
            for (x, expected) in [(685.0, 0), (731.0, 1), (777.0, 2)] {
                let ctx = egui::Context::default();
                crate::design::apply(&ctx, theme);
                let input = |events| egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input(vec![]), |ui| {
                    titlebar(ui, None, |_| {});
                });
                output.textures_delta.clear();
                let pos = egui::pos2(x, 20.0);
                let event = |pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                let mut output = ctx.run_ui(
                    input(vec![egui::Event::PointerMoved(pos), event(true)]),
                    |ui| {
                        titlebar(ui, None, |_| {});
                    },
                );
                output.textures_delta.clear();
                let mut output = ctx.run_ui(input(vec![event(false)]), |ui| {
                    titlebar(ui, None, |_| {});
                });
                let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
                assert!(
                    commands.iter().any(|c| matches!(
                        (expected, c),
                        (0, egui::ViewportCommand::Minimized(true))
                            | (1, egui::ViewportCommand::Maximized(true))
                            | (2, egui::ViewportCommand::Close)
                    )),
                    "caption action {expected} did not produce its command"
                );
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn caption_glyph_contrast_exceeds_accessible_text_threshold() {
        fn luminance(c: Color32) -> f32 {
            let linear = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
        }
        for dark in [false, true] {
            let p = Palette::new(dark);
            for (ink, background) in [(p.ink, p.ground), (p.ink, p.hover), (p.ground, p.bad)] {
                let (a, b) = (luminance(ink), luminance(background));
                assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
            }
        }
    }
}
