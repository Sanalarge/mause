use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, StrokeKind, Vec2,
    epaint::Mesh,
};

pub const ON_ACCENT: Color32 = Color32::from_rgb(0x12, 0x12, 0x14);
pub const MAX_CONTENT: f32 = 1040.0;
pub const BODY: f32 = 15.0;
pub const SMALL: f32 = 14.0;
pub const META: f32 = 13.0;
pub const LEAD: f32 = 17.0;
pub const TITLE: f32 = 22.0;
pub const CONTROL: f32 = 38.0;
pub const RADIUS: u8 = 8;
pub const RADIUS_BUTTON: u8 = 12;
pub const RADIUS_PANEL: u8 = 20;

#[derive(Clone, Copy)]
pub struct Palette {
    pub ground: Color32,
    pub surface: Color32,
    pub inset: Color32,
    pub hover: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub muted: Color32,
    pub rule: Color32,
    pub rule_strong: Color32,
    pub field: Color32,
    pub selected: Color32,
    pub signal: Color32,
    pub accent: Color32,
    pub accent_edge: Color32,
    pub grad_a: Color32,
    pub grad_b: Color32,
    pub key_face: Color32,
    pub graphite: Color32,
    pub graphite_ink: Color32,
    pub ok: Color32,
    pub off: Color32,
    pub warn: Color32,
    pub bad: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        let orange = Color32::from_rgb(0xff, 0x95, 0x00);
        let orange_edge = Color32::from_rgb(0xe0, 0x7f, 0x00);
        let lime = Color32::from_rgb(0xc6, 0xf4, 0x32);
        if dark {
            let text2 = Color32::from_rgb(0xc2, 0xcb, 0xda);
            Self {
                ground: Color32::from_rgb(0x0d, 0x11, 0x1c),
                surface: Color32::from_rgb(0x12, 0x18, 0x26),
                inset: Color32::from_rgb(0x18, 0x21, 0x33),
                hover: Color32::from_rgb(0x1d, 0x29, 0x40),
                ink: Color32::from_rgb(0xf5, 0xf7, 0xfb),
                ink2: text2,
                muted: Color32::from_rgb(0xa7, 0xb1, 0xc2),
                rule: Color32::from_rgb(0x23, 0x2d, 0x40),
                rule_strong: Color32::from_rgb(0x31, 0x3d, 0x54),
                field: Color32::from_rgb(0x55, 0x63, 0x78),
                selected: Color32::from_rgb(0x1b, 0x27, 0x3c),
                signal: orange,
                accent: orange,
                accent_edge: orange_edge,
                grad_a: Color32::from_rgb(0xc2, 0x41, 0x0c),
                grad_b: orange,
                key_face: Color32::from_rgb(0x18, 0x21, 0x33),
                graphite: Color32::from_rgb(0x08, 0x0c, 0x15),
                graphite_ink: Color32::from_rgb(0xf5, 0xf7, 0xfb),
                ok: lime,
                off: Color32::from_rgb(0xff, 0x6a, 0x3d),
                warn: text2,
                bad: Color32::from_rgb(0xff, 0x6b, 0x6b),
            }
        } else {
            let text2 = Color32::from_rgb(0x45, 0x46, 0x4c);
            let green = Color32::from_rgb(0x3f, 0x62, 0x12);
            Self {
                ground: Color32::from_rgb(0xf1, 0xf2, 0xee),
                surface: Color32::from_rgb(0xff, 0xff, 0xff),
                inset: Color32::from_rgb(0xf0, 0xf1, 0xec),
                hover: Color32::from_rgb(0xe6, 0xe8, 0xe1),
                ink: Color32::from_rgb(0x14, 0x14, 0x16),
                ink2: text2,
                muted: Color32::from_rgb(0x5c, 0x5d, 0x64),
                rule: Color32::from_rgb(0xe3, 0xe4, 0xde),
                rule_strong: Color32::from_rgb(0xd2, 0xd4, 0xcc),
                field: Color32::from_rgb(0xa9, 0xab, 0xa3),
                selected: Color32::from_rgba_unmultiplied(0xff, 0x95, 0x00, 0x33),
                signal: Color32::from_rgb(0x9a, 0x4a, 0x00),
                accent: orange,
                accent_edge: orange_edge,
                grad_a: Color32::from_rgb(0xc2, 0x41, 0x0c),
                grad_b: orange,
                key_face: Color32::from_rgb(0xff, 0xff, 0xff),
                graphite: Color32::from_rgb(0x14, 0x14, 0x16),
                graphite_ink: Color32::from_rgb(0xf4, 0xf4, 0xf5),
                ok: green,
                off: Color32::from_rgb(0xb4, 0x23, 0x18),
                warn: text2,
                bad: Color32::from_rgb(0xc4, 0x2a, 0x3a),
            }
        }
    }
    pub fn of(ui: &egui::Ui) -> Self {
        Self::new(ui.visuals().dark_mode)
    }
}

pub fn apply(ctx: &egui::Context, theme: u8) {
    ctx.set_theme(match theme {
        1 => egui::ThemePreference::Dark,
        2 => egui::ThemePreference::Light,
        _ => egui::ThemePreference::System,
    });
    ctx.all_styles_mut(|s| {
        let p = Palette::new(s.visuals.dark_mode);
        use egui::TextStyle::*;
        for (style, size) in [
            (Heading, TITLE),
            (Body, BODY),
            (Button, BODY),
            (Small, SMALL),
        ] {
            let family = if matches!(style, Heading | Button) {
                egui::FontFamily::Name("semibold".into())
            } else {
                egui::FontFamily::Proportional
            };
            s.text_styles.insert(style, FontId::new(size, family));
        }
        s.text_styles.insert(Monospace, FontId::monospace(SMALL));
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(14.0, 8.0);
        s.spacing.interact_size = Vec2::new(40.0, CONTROL);
        s.spacing.combo_height = 320.0;
        s.animation_time = 0.0;
        s.visuals.panel_fill = p.ground;
        s.visuals.window_fill = p.inset;
        s.visuals.window_stroke = Stroke::new(1.0, p.rule_strong);
        s.visuals.window_corner_radius = CornerRadius::same(RADIUS_PANEL);
        s.visuals.menu_corner_radius = CornerRadius::same(RADIUS_PANEL);
        s.visuals.faint_bg_color = p.surface;
        s.visuals.extreme_bg_color = p.inset;
        s.visuals.code_bg_color = p.inset;
        s.visuals.override_text_color = Some(p.ink);
        s.visuals.weak_text_color = Some(p.muted);
        s.visuals.hyperlink_color = p.signal;
        s.visuals.selection.bg_fill = p.selected;
        s.visuals.selection.stroke = Stroke::new(1.5, p.signal);
        s.visuals.text_edit_bg_color = Some(p.inset);
        s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.rule);
        s.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS);
        for w in [
            &mut s.visuals.widgets.inactive,
            &mut s.visuals.widgets.hovered,
            &mut s.visuals.widgets.active,
            &mut s.visuals.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(RADIUS);
            w.fg_stroke = Stroke::new(1.0, p.ink);
            w.bg_stroke = Stroke::new(1.0, p.rule_strong);
            w.expansion = 0.0;
        }
        s.visuals.widgets.inactive.weak_bg_fill = p.inset;
        s.visuals.widgets.inactive.bg_fill = p.inset;
        s.visuals.widgets.hovered.weak_bg_fill = p.hover;
        s.visuals.widgets.hovered.bg_fill = p.hover;
        s.visuals.widgets.active.weak_bg_fill = p.selected;
        s.visuals.widgets.active.bg_fill = p.selected;
        s.visuals.widgets.open.weak_bg_fill = p.selected;
        s.visuals.widgets.open.bg_fill = p.selected;
        s.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.field);
        s.visuals.widgets.active.bg_stroke = Stroke::new(1.5, p.signal);
        s.visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.signal);
    });
}

pub fn install_fonts(ctx: &egui::Context) {
    let font = |bytes: &'static [u8]| std::sync::Arc::new(egui::FontData::from_static(bytes));
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "poppins".into(),
        font(include_bytes!("../assets/fonts/Poppins-Regular.ttf")),
    );
    fonts.font_data.insert(
        "poppins-medium".into(),
        font(include_bytes!("../assets/fonts/Poppins-Medium.ttf")),
    );
    fonts.font_data.insert(
        "poppins-semibold".into(),
        font(include_bytes!("../assets/fonts/Poppins-SemiBold.ttf")),
    );
    fonts.font_data.insert(
        "phosphor".into(),
        font(include_bytes!("../assets/fonts/Phosphor-Regular.ttf")),
    );
    fonts.families.insert(
        egui::FontFamily::Name("icons".into()),
        vec!["phosphor".into()],
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "poppins".into());
    let base = fonts.families[&egui::FontFamily::Proportional].clone();
    fonts
        .families
        .insert(egui::FontFamily::Name("body".into()), base.clone());
    for (family, data) in [
        ("medium", "poppins-medium"),
        ("semibold", "poppins-semibold"),
        ("wordmark", "poppins-semibold"),
    ] {
        let mut list = base.clone();
        list.insert(0, data.into());
        fonts
            .families
            .insert(egui::FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);
}

pub fn wordmark(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("wordmark".into()))
}

pub fn brand(
    painter: &egui::Painter,
    left_center: egui::Pos2,
    logo: Option<egui::TextureId>,
    p: Palette,
) {
    if let Some(id) = logo {
        painter.image(
            id,
            Rect::from_center_size(left_center + Vec2::new(11.0, 0.0), Vec2::splat(22.0)),
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    painter.text(
        left_center + Vec2::new(31.0, -1.0),
        Align2::LEFT_CENTER,
        "mause",
        wordmark(18.0),
        p.ink,
    );
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("medium".into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

pub fn strong(text: impl Into<String>, size: f32) -> egui::RichText {
    egui::RichText::new(text).font(semibold(size))
}

#[derive(Clone, Copy, PartialEq)]
pub enum KeyKind {
    Primary,
    Plain,
    Danger,
}

pub fn key(
    ui: &mut egui::Ui,
    text: &str,
    kind: KeyKind,
    enabled: bool,
    min_width: f32,
) -> Response {
    key_with(ui, None, text, text, kind, enabled, min_width)
}

pub fn key_with(
    ui: &mut egui::Ui,
    icon_kind: Option<Icon>,
    text: &str,
    accessible_name: &str,
    kind: KeyKind,
    enabled: bool,
    min_width: f32,
) -> Response {
    let p = Palette::of(ui);
    let font = medium(13.5);
    let icon_room = if icon_kind.is_some() { 24.0 } else { 0.0 };
    let ink = match (enabled, kind) {
        (false, _) => p.muted,
        (true, KeyKind::Primary) => ON_ACCENT,
        (true, KeyKind::Danger) => p.bad,
        (true, KeyKind::Plain) => p.ink,
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let width = (galley.size().x + 32.0 + icon_room)
        .max(min_width)
        .min(ui.available_width().max(60.0));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, CONTROL), sense);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, accessible_name));
    if ui.is_rect_visible(rect) {
        let hot = enabled && response.hovered();
        let pressed = enabled && response.is_pointer_button_down_on();
        let (fill, border) = match (enabled, kind) {
            (false, _) => (Color32::TRANSPARENT, p.rule_strong),
            (true, KeyKind::Primary) if pressed => (p.accent_edge, p.accent_edge),
            (true, KeyKind::Primary) if hot => (p.accent_edge, p.accent_edge),
            (true, KeyKind::Primary) => (p.accent, p.accent),
            (true, _) if pressed => (p.rule_strong, p.rule_strong),
            (true, _) if hot => (p.hover, p.field),
            (true, _) => (p.inset, p.rule_strong),
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            RADIUS_BUTTON,
            fill,
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        let text_pos = rect.center() - galley.size() / 2.0 + Vec2::new(icon_room / 2.0, 0.0);
        if let Some(kind) = icon_kind {
            let at = egui::pos2(text_pos.x - 12.0, rect.center().y);
            icon(painter, at, kind, ink, 17.0);
        }
        painter.galley(text_pos, galley, ink);
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                RADIUS_BUTTON + 2,
                Stroke::new(2.0, p.signal),
                StrokeKind::Outside,
            );
        }
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn gradient(painter: &egui::Painter, rect: Rect, a: Color32, b: Color32, vertical: bool) {
    let (tr, bl) = if vertical { (a, b) } else { (b, a) };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), tr);
    mesh.colored_vertex(rect.right_bottom(), b);
    mesh.colored_vertex(rect.left_bottom(), bl);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

pub fn progress(ui: &mut egui::Ui, fraction: f32, width: f32) -> Response {
    let p = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let track = Rect::from_center_size(rect.center(), Vec2::new(width, 5.0));
    let painter = ui.painter();
    painter.rect_filled(track, 3, p.rule_strong);
    let f = fraction.clamp(0.0, 1.0);
    if f > 0.0 {
        let filled = Rect::from_min_size(track.min, Vec2::new(track.width() * f, track.height()));
        let end = lerp_color(p.grad_a, p.grad_b, f);
        gradient(
            &painter.with_clip_rect(filled),
            filled,
            p.grad_a,
            end,
            false,
        );
    }
    response
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

pub fn toggle(ui: &mut egui::Ui, on: &mut bool, label: &str, enabled: bool) -> Response {
    let p = Palette::of(ui);
    let size = Vec2::new(40.0, 22.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(size.x, CONTROL), sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let state = *on && enabled;
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, state, label));
    if ui.is_rect_visible(rect) {
        let track = Rect::from_center_size(rect.center(), size);
        let (fill, border) = match (enabled, state) {
            (false, _) => (Color32::TRANSPARENT, p.rule_strong),
            (true, true) => (p.ok, p.ok),
            (true, false) => (p.inset, p.off),
        };
        let painter = ui.painter();
        painter.rect(
            track,
            11,
            fill,
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        let knob_x = if state {
            track.right() - 11.0
        } else {
            track.left() + 11.0
        };
        let knob_fill = if !enabled {
            p.rule_strong
        } else if state {
            ON_ACCENT
        } else {
            p.off
        };
        painter.circle_filled(egui::pos2(knob_x, track.center().y), 7.5, knob_fill);
        if response.has_focus() {
            painter.rect_stroke(
                track.expand(2.0),
                13,
                Stroke::new(2.0, p.signal),
                StrokeKind::Outside,
            );
        }
    }
    response
}

pub const KEYCAP: f32 = 64.0;

pub fn keycap(
    ui: &mut egui::Ui,
    icon_kind: Icon,
    name: &str,
    on: bool,
    live: bool,
    enabled: bool,
) -> Response {
    keycap_with(ui, icon_kind, name, on, false, live, enabled)
}

pub fn keycap_with(
    ui: &mut egui::Ui,
    icon_kind: Icon,
    name: &str,
    on: bool,
    peer_off: bool,
    live: bool,
    enabled: bool,
) -> Response {
    let peer_off = peer_off && !on;
    let p = Palette::of(ui);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(KEYCAP), sense);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, enabled, on, name));
    if ui.is_rect_visible(rect) {
        let hot = enabled && response.hovered();
        let (fill, border, ink) = match (enabled, on) {
            (false, _) => (Color32::TRANSPARENT, p.rule, p.rule_strong),
            (true, true) => (p.inset, p.ok, p.ok),
            (true, false) if peer_off => (if hot { p.hover } else { p.inset }, p.signal, p.ink2),
            (true, false) if hot => (p.hover, p.field, p.ink),
            (true, false) => (p.inset, p.rule_strong, p.ink2),
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            RADIUS_BUTTON,
            fill,
            Stroke::new(
                if (on || peer_off) && enabled {
                    1.5
                } else {
                    1.0
                },
                border,
            ),
            StrokeKind::Inside,
        );
        icon(
            painter,
            rect.center() - Vec2::new(0.0, 8.0),
            icon_kind,
            ink,
            22.0,
        );
        let label = if !enabled {
            p.muted
        } else if on {
            p.ok
        } else if peer_off {
            p.signal
        } else {
            p.muted
        };
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 14.0),
            Align2::CENTER_CENTER,
            name,
            if on {
                medium(11.0)
            } else {
                FontId::proportional(11.0)
            },
            label,
        );
        if enabled && (live || !on) {
            painter.circle_filled(
                rect.right_top() + Vec2::new(-9.0, 9.0),
                3.5,
                if on {
                    p.ok
                } else if peer_off {
                    p.signal
                } else {
                    p.off
                },
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                RADIUS_BUTTON + 2,
                Stroke::new(2.0, p.signal),
                StrokeKind::Outside,
            );
        }
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn radio(ui: &mut egui::Ui, selected: bool, label: &str) -> Response {
    let p = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(24.0, CONTROL), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center = rect.center();
        let ring = if selected || response.hovered() {
            p.signal
        } else {
            p.field
        };
        painter.circle(center, 9.0, p.key_face, Stroke::new(1.5, ring));
        if selected {
            painter.circle_filled(center, 4.5, p.accent);
        }
        if response.has_focus() {
            painter.circle_stroke(center, 12.0, Stroke::new(2.0, p.signal));
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn segmented(ui: &mut egui::Ui, id: &str, options: &[&str], selected: usize) -> Option<usize> {
    segmented_impl(ui, id, options, selected, false)
}

fn segmented_impl(
    ui: &mut egui::Ui,
    id: &str,
    options: &[&str],
    selected: usize,
    reselect: bool,
) -> Option<usize> {
    let p = Palette::of(ui);
    let font = semibold(14.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|o| {
            ui.painter()
                .layout_no_wrap((*o).to_owned(), font.clone(), p.ink)
                .size()
                .x
                + 28.0
        })
        .collect();
    let total = widths.iter().sum::<f32>() + 8.0;
    let (tray, _) = ui.allocate_exact_size(Vec2::new(total, CONTROL + 2.0), Sense::hover());
    ui.painter().rect(
        tray,
        RADIUS + 1,
        p.inset,
        Stroke::new(1.0, p.rule),
        StrokeKind::Inside,
    );
    let mut picked = None;
    let mut x = tray.left() + 4.0;
    for (i, (option, w)) in options.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(
            egui::pos2(x, tray.top() + 4.0),
            Vec2::new(*w, CONTROL - 6.0),
        );
        x += w;
        let response = ui.interact(rect, ui.scope_id().with((id, i)), Sense::click());
        let chosen = i == selected;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, true, chosen, *option)
        });
        let painter = ui.painter();
        if chosen {
            painter.rect_filled(rect, RADIUS - 1, p.selected);
        } else if response.hovered() {
            painter.rect_filled(rect, RADIUS - 1, p.hover);
        }
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            *option,
            font.clone(),
            if chosen { p.signal } else { p.ink2 },
        );
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(1.0),
                RADIUS,
                Stroke::new(2.0, p.signal),
                StrokeKind::Outside,
            );
        }
        if response.clicked() && (!chosen || reselect) {
            picked = Some(i);
        }
    }
    picked
}

pub fn rule(ui: &mut egui::Ui) {
    let p = Palette::of(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.rule));
}

#[derive(Clone, Copy)]
pub enum Icon {
    Home,
    Share,
    Screens,
    Folder,
    File,
    Refresh,
    Download,
    Audio,
    Files,
    Settings,
    Log,
    Shield,
    Plus,
    Upload,
    Link,
    Search,
    Off,
    Alert,
    Keyboard,
    Mic,
    Clipboard,
    Lock,
    Usb,
    GitHub,
    LinkedIn,
    Windows,
    Apple,
    Linux,
    Android,
    Bell,
    CaretLeft,
    CaretRight,
    CaretUp,
    CaretDown,
    Trash,
    FolderPlus,
    More,
    Check,
    Cpu,
    Memory,
    GraphicsCard,
    HardDrive,
    Network,
    Info,
    Minimize,
    Maximize,
    Restore,
    Close,
}

pub fn icon(painter: &egui::Painter, center: egui::Pos2, kind: Icon, color: Color32, size: f32) {
    let glyph = match kind {
        Icon::Home => '\u{e2c2}',
        Icon::Share => '\u{e0a0}',
        Icon::Screens => '\u{e32e}',
        Icon::Folder => '\u{e24a}',
        Icon::File => '\u{e230}',
        Icon::Refresh => '\u{e036}',
        Icon::Download => '\u{e20c}',
        Icon::Audio => '\u{e44a}',
        Icon::Files => '\u{e25a}',
        Icon::Settings => '\u{e272}',
        Icon::Log => '\u{eae8}',
        Icon::Shield => '\u{e40c}',
        Icon::Plus => '\u{e3d4}',
        Icon::Upload => '\u{e4c0}',
        Icon::Link => '\u{e2e6}',
        Icon::Search => '\u{e30c}',
        Icon::Off => '\u{e3da}',
        Icon::Alert => '\u{e4e0}',
        Icon::Keyboard => '\u{e2d8}',
        Icon::Mic => '\u{e326}',
        Icon::Clipboard => '\u{e198}',
        Icon::Lock => '\u{e308}',
        Icon::Usb => '\u{e956}',
        Icon::GitHub => '\u{e576}',
        Icon::LinkedIn => '\u{e2ee}',
        Icon::Windows => '\u{e692}',
        Icon::Apple => '\u{e516}',
        Icon::Linux => '\u{eb02}',
        Icon::Android => '\u{e008}',
        Icon::Bell => '\u{e0ce}',
        Icon::CaretLeft => '\u{e138}',
        Icon::CaretRight => '\u{e13a}',
        Icon::CaretUp => '\u{e13c}',
        Icon::CaretDown => '\u{e136}',
        Icon::Trash => '\u{e4a6}',
        Icon::FolderPlus => '\u{e258}',
        Icon::More => '\u{e206}',
        Icon::Check => '\u{e182}',
        Icon::Cpu => '\u{e610}',
        Icon::Memory => '\u{e9c4}',
        Icon::GraphicsCard => '\u{e612}',
        Icon::HardDrive => '\u{e29e}',
        Icon::Network => '\u{edde}',
        Icon::Info => '\u{e2ce}',
        Icon::Minimize => '\u{e32a}',
        Icon::Maximize => '\u{e45e}',
        Icon::Restore => '\u{e1cc}',
        Icon::Close => '\u{e4f6}',
    };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::new(size, egui::FontFamily::Name("icons".into())),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn icon_button_exposes_its_contextual_accessible_name() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        install_fonts(&ctx);
        apply(&ctx, 1);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(320.0, 100.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            key_with(
                ui,
                Some(Icon::GitHub),
                "GitHub",
                "Muhammed Taymur GitHub profilini aç",
                KeyKind::Plain,
                true,
                120.0,
            );
        });
        let update = output
            .platform_output
            .accesskit_update
            .take()
            .expect("AccessKit ağacı oluşturulmalı");
        assert!(
            update
                .nodes
                .iter()
                .any(|(_, node)| { node.label() == Some("Muhammed Taymur GitHub profilini aç") })
        );
        output.textures_delta.clear();
    }

    #[test]
    fn every_text_colour_passes_wcag_aa_on_every_surface_in_both_themes() {
        for dark in [true, false] {
            let p = Palette::new(dark);
            for (name, text) in [
                ("text", p.ink),
                ("text-2", p.ink2),
                ("text-3", p.muted),
                ("accent-text", p.signal),
                ("danger", p.bad),
                ("ok", p.ok),
                ("off", p.off),
            ] {
                for (bg_name, bg) in [
                    ("bg", p.ground),
                    ("surface", p.surface),
                    ("raised", p.inset),
                    ("hover", p.hover),
                ] {
                    let ratio = contrast(text, bg);
                    assert!(ratio >= 4.5, "dark={dark} {name} on {bg_name}: {ratio:.2}");
                }
            }
            assert!(
                contrast(ON_ACCENT, p.accent) >= 4.5,
                "dark={dark} dark text on accent"
            );
            assert!(
                contrast(p.graphite_ink, p.graphite) >= 4.5,
                "dark={dark} log text"
            );
        }
    }
}
