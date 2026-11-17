// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 wulflo

use crate::analysis;
use crate::bridge::{IndexEvent, UiReceiver};
use crate::config::Config;
use crate::player::{self, PlayOrder};
use crate::store::Store;
use gtk4::glib;
use gtk4::prelude::*;
use std::time::Duration;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Label, Orientation,
    PolicyType, ProgressBar, ScrolledWindow,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Límites del control de velocidad. El mínimo es bajo para poder leer muy
/// despacio; el máximo mantiene la marquesina legible.
const MIN_SPEED: f64 = 0.05;
const MAX_SPEED: f64 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Title,
}

pub struct PlayerState {
    pub order: PlayOrder,
    pub current_path: Option<String>,
    pub current_index: usize,
    pub lines: Vec<String>,
    pub line_idx: usize,
    pub phase: Phase,
    pub paused: bool,
    pub speed: f64,
    pub include_code: bool,
    pub top_words: Vec<(String, String, i64)>,
    pub showing_controls: bool,
}

impl PlayerState {
    pub fn new(config: &Config) -> Self {
        let order = if config.order == "random" {
            PlayOrder::Random
        } else {
            PlayOrder::Alphabetical
        };
        Self {
            order,
            current_path: None,
            current_index: 0,
            lines: Vec::new(),
            line_idx: 0,
            phase: Phase::Playing,
            paused: false,
            speed: config.speed.clamp(MIN_SPEED, MAX_SPEED),
            include_code: config.include_code_blocks,
            top_words: Vec::new(),
            showing_controls: true,
        }
    }
}

pub struct UiState {
    pub config: Rc<RefCell<Config>>,
    pub store: Arc<Store>,
    pub player: Rc<RefCell<PlayerState>>,
    /// Contenedor de la línea actual. Cada palabra es un `Label` propio para
    /// poder darle tamaño y glow independientes sin perder el contorno blanco.
    pub line_box: GtkBox,
    pub scroller: ScrolledWindow,
    pub spacer: GtkBox,
    pub spacer_end: GtkBox,
    pub progress: ProgressBar,
    pub controls: GtkBox,
    pub play_btn: Button,
    pub fs_btn: Button,
    pub theme_btn: Button,
    pub speed_scale: gtk4::Scale,
    pub speed_value: Label,
    pub window: ApplicationWindow,
    pub cancel: Arc<AtomicBool>,
    pub root_tx: std::sync::mpsc::Sender<std::path::PathBuf>,
    pub events_queue: Arc<std::sync::Mutex<Vec<IndexEvent>>>,
    pub scroll_source: RefCell<Option<gtk4::TickCallbackId>>,
    pub pending_scroll: RefCell<Option<glib::SourceId>>,
    pub anim_elapsed: RefCell<f64>,
    pub last_tick: RefCell<std::time::Instant>,
    pub anim_from: RefCell<f64>,
    pub anim_to: RefCell<f64>,
    pub anim_duration: RefCell<u64>,
    /// `true` mientras se muestra el hueco en blanco entre dos líneas.
    pub in_gap: RefCell<bool>,
    pub anim_line: RefCell<String>,
    pub top_n: usize,
    pub base_font: i32,
    pub min_font: i32,
    pub max_font: i32,
    pub font_family: String,
    pub accent_color: String,
    pub line_gap_ms: u64,
    pub css: CssProvider,
}

/// Aplica la preferencia de tema ("auto", "dark", "light") al conjunto de la
/// aplicación: ventana, diálogos y controles GTK.
fn apply_theme_preference(theme: &str) {
    use libadwaita::{ColorScheme, StyleManager};

    let scheme = match theme {
        "dark" => ColorScheme::ForceDark,
        "light" => ColorScheme::ForceLight,
        _ => ColorScheme::Default,
    };
    StyleManager::default().set_color_scheme(scheme);
}

fn theme_is_dark() -> bool {
    libadwaita::StyleManager::default().is_dark()
}

type Rgb = (u8, u8, u8);

fn parse_hex_color(input: &str) -> Option<Rgb> {
    let s = input.trim().trim_start_matches('#');
    if !s.is_ascii() {
        return None;
    }
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()?;
            let g = u8::from_str_radix(&s[1..2], 16).ok()?;
            let b = u8::from_str_radix(&s[2..3], 16).ok()?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn mix_color(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let blend = |x: u8, y: u8| (x as f64 * (1.0 - t) + y as f64 * t).round() as u8;
    (blend(a.0, b.0), blend(a.1, b.1), blend(a.2, b.2))
}

fn hex_color(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn rgba_color(c: Rgb, alpha: f64) -> String {
    format!(
        "rgba({}, {}, {}, {:.2})",
        c.0,
        c.1,
        c.2,
        alpha.clamp(0.0, 1.0)
    )
}

fn rgba_to_hex(color: &gtk4::gdk::RGBA) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    hex_color((channel(color.red()), channel(color.green()), channel(color.blue())))
}

/// Genera el bloque `@define-color` a partir del acento elegido. El fondo, el
/// glow y los controles derivan del mismo color para que el cambio sea global.
fn accent_definitions(accent: &str) -> String {
    let base = parse_hex_color(accent).unwrap_or((0xe5, 0x30, 0x4a));
    let white = (255, 255, 255);
    let black = (0, 0, 0);

    let soft = mix_color(base, white, 0.45);
    let bright = mix_color(base, white, 0.22);
    let deep = mix_color(base, black, 0.32);

    let bg_center = mix_color(base, black, 0.80);
    let bg = mix_color(base, black, 0.91);
    let bg_deep = mix_color(base, black, 0.97);
    let light_center = mix_color(base, white, 0.95);
    let light = mix_color(base, white, 0.90);
    let light_deep = mix_color(base, white, 0.86);

    let outline_dark = mix_color(base, black, 0.93);

    format!(
        r#"
        @define-color accent {accent};
        @define-color accent_soft {soft};
        @define-color accent_bright {bright};
        @define-color accent_deep {deep};
        @define-color accent_a65 {a65};
        @define-color accent_a55 {a55};
        @define-color accent_a45 {a45};
        @define-color accent_a30 {a30};
        @define-color accent_a25 {a25};
        @define-color accent_a18 {a18};
        @define-color accent_a12 {a12};
        @define-color bg_dark {bg};
        @define-color bg_dark_center {bg_center};
        @define-color bg_dark_deep {bg_deep};
        @define-color bg_light {light};
        @define-color bg_light_center {light_center};
        @define-color bg_light_deep {light_deep};
        @define-color glass_dark {glass_dark};
        @define-color glass_light {glass_light};
        @define-color outline_dark {outline_dark};
        @define-color outline_light #ffffff;
        @define-color glow_a {glow_a};
        @define-color glow_b {glow_b};
        @define-color glow_c {glow_c};
        @define-color glow_hi_a {glow_hi_a};
        @define-color glow_hi_b {glow_hi_b};
        @define-color glow_hi_c {glow_hi_c};
        "#,
        accent = hex_color(base),
        soft = hex_color(soft),
        bright = hex_color(bright),
        deep = hex_color(deep),
        a65 = rgba_color(base, 0.65),
        a55 = rgba_color(base, 0.55),
        a45 = rgba_color(base, 0.45),
        a30 = rgba_color(base, 0.30),
        a25 = rgba_color(base, 0.25),
        a18 = rgba_color(base, 0.18),
        a12 = rgba_color(base, 0.12),
        bg = hex_color(bg),
        bg_center = hex_color(bg_center),
        bg_deep = hex_color(bg_deep),
        light = hex_color(light),
        light_center = hex_color(light_center),
        light_deep = hex_color(light_deep),
        glass_dark = rgba_color(bg, 0.55),
        glass_light = rgba_color(white, 0.55),
        outline_dark = hex_color(outline_dark),
        glow_a = rgba_color(bright, 0.85),
        glow_b = rgba_color(base, 0.45),
        glow_c = rgba_color(base, 0.22),
        glow_hi_a = rgba_color(bright, 0.95),
        glow_hi_b = rgba_color(base, 0.65),
        glow_hi_c = rgba_color(base, 0.35),
    )
}

fn css_escape_font(font: &str) -> String {
    font.trim().replace('\\', "\\\\").replace('"', "\\\"")
}

fn font_stack(font_family: &str) -> String {
    let family = css_escape_font(font_family);
    if family.is_empty() {
        "\"FantasqueSansM Nerd Font\", \"MesloLGS Nerd Font\", monospace".to_string()
    } else {
        format!(
            "\"{family}\", \"FantasqueSansM Nerd Font\", \"MesloLGS Nerd Font\", monospace"
        )
    }
}

fn build_css(accent: &str, font_family: &str, base_font: i32) -> String {
    let colors = accent_definitions(accent);
    CSS_TEMPLATE
        .replace("__COLORS__", &colors)
        .replace("__FONT_FAMILY__", &font_stack(font_family))
        .replace("__BASE_FONT__", &base_font.clamp(6, 200).to_string())
}

/// Carga de nuevo la hoja de estilos dinámica (acento, fuente y tamaño base).
fn reload_css(st: &UiState) {
    st.css
        .load_from_string(&build_css(&st.accent_color, &st.font_family, st.base_font));
}

/// Sincroniza las clases CSS de la ventana con el tema efectivo.
fn sync_theme_classes(st: &UiState) {
    let dark = theme_is_dark();
    if dark {
        st.window.remove_css_class("tm-light");
        st.window.add_css_class("tm-dark");
    } else {
        st.window.remove_css_class("tm-dark");
        st.window.add_css_class("tm-light");
    }
    let (icon, tip) = if dark {
        ("weather-clear-symbolic", "Cambiar a modo claro")
    } else {
        ("weather-clear-night-symbolic", "Cambiar a modo oscuro")
    };
    st.theme_btn.set_icon_name(icon);
    st.theme_btn.set_tooltip_text(Some(tip));
    st.line_box.queue_resize();
}

fn sync_play_button(st: &UiState) {
    let paused = st.player.borrow().paused;
    if paused {
        st.play_btn.set_icon_name("media-playback-start-symbolic");
        st.play_btn.set_tooltip_text(Some("Reanudar (Espacio)"));
    } else {
        st.play_btn.set_icon_name("media-playback-pause-symbolic");
        st.play_btn.set_tooltip_text(Some("Pausa (Espacio)"));
    }
}

fn sync_fs_button(st: &UiState) {
    if st.window.is_fullscreen() {
        st.fs_btn.set_icon_name("view-restore-symbolic");
    } else {
        st.fs_btn.set_icon_name("view-fullscreen-symbolic");
    }
}

fn sync_speed_controls(st: &UiState) {
    let speed = st.player.borrow().speed.clamp(MIN_SPEED, MAX_SPEED);
    if (st.speed_scale.value() - speed).abs() > f64::EPSILON {
        st.speed_scale.set_value(speed);
    }
    st.speed_value.set_text(&format!("{:.2}×", speed));
}

pub fn run_app(config: Config) {
    // libadwaita nos da el tema del sistema y el esquema de color global.
    if let Err(err) = libadwaita::init() {
        tracing::warn!("no se pudo inicializar libadwaita: {err}");
    }

    use crate::bridge::ui_channel;
    use crate::indexer::runner::{IndexConfig, run_supervisor};
    use crate::indexer::watcher::WatchConfig;

    let (tx, rx) = ui_channel(256);
    let (root_tx, root_rx) = std::sync::mpsc::channel::<std::path::PathBuf>();

    let config_rc = Rc::new(RefCell::new(config.clone()));
    let store = Arc::new(Store::open(&Config::db_path()).unwrap_or_else(|_| Store::open_in_memory().unwrap()));
    let cancel = Arc::new(AtomicBool::new(false));
    let player_state = Rc::new(RefCell::new(PlayerState::new(&config)));

    // Hilo supervisor: indexa y vigila la carpeta actual, y reinicia el
    // trabajo cuando el usuario abre otra carpeta.
    let store_i = store.clone();
    let cfg_i = config.clone();
    let tx_i = tx.clone();
    let cancel_i = cancel.clone();
    std::thread::spawn(move || {
        let root = cfg_i.folder.clone().unwrap_or_else(|| std::path::PathBuf::from("."));
        let icfg = IndexConfig {
            root: root.clone(),
            max_size: cfg_i.max_file_size,
            stopwords: cfg_i.stopwords.clone(),
            min_len: cfg_i.min_word_len,
            include_code: cfg_i.include_code_blocks,
            top_n: cfg_i.top_n,
            pool: crate::indexer::worker::pool_size(cfg_i.worker_threads),
        };
        let wcfg = WatchConfig {
            root,
            max_size: cfg_i.max_file_size,
            stopwords: cfg_i.stopwords.clone(),
            min_len: cfg_i.min_word_len,
            include_code: cfg_i.include_code_blocks,
            top_n: cfg_i.top_n,
        };
        run_supervisor(store_i, icfg, wcfg, tx_i, cancel_i, root_rx);
    });

    let app = Application::builder()
        .application_id("org.wulflo.MrReader")
        .build();

    let store_c = store.clone();
    let player_c = player_state.clone();
    let cancel_c = cancel.clone();
    let config_rc_c = config_rc.clone();
    let root_tx_c = root_tx.clone();
    app.connect_activate(move |app| {
        apply_theme_preference(&config_rc_c.borrow().theme);
        build_window(
            app,
            config_rc_c.clone(),
            store_c.clone(),
            player_c.clone(),
            cancel_c.clone(),
            rx.clone(),
            root_tx_c.clone(),
        );
    });

    app.run_with_args(&["mr-reader"]);
    cancel.store(true, Ordering::Relaxed);
    // Cerrar el canal para que el supervisor termine si está esperando.
    drop(root_tx);
}

fn build_window(
    app: &Application,
    config_rc: Rc<RefCell<Config>>,
    store: Arc<Store>,
    player: Rc<RefCell<PlayerState>>,
    cancel: Arc<AtomicBool>,
    rx: UiReceiver,
    root_tx: std::sync::mpsc::Sender<std::path::PathBuf>,
) {
    let config = config_rc.borrow().clone();

    // Barra de título mínima: sólo los botones de minimizar, maximizar y
    // cerrar. Sin texto, para no mostrar "Mr. Reader", pero con decoraciones
    // nativas para poder mover/redimensionar la ventana.
    let window = ApplicationWindow::builder()
        .application(app)
        .title("")
        .decorated(true)
        .default_width(800)
        .default_height(600)
        .build();

    let header = gtk4::HeaderBar::new();
    header.set_show_title_buttons(true);
    header.set_decoration_layout(Some("icon:minimize,maximize,close"));
    header.add_css_class("tm-titlebar");
    window.set_titlebar(Some(&header));

    // CSS dinámico (acento, familia tipográfica y tamaño base).
    let css = CssProvider::new();
    css.load_from_string(&build_css(
        &config.accent_color,
        &config.font_family,
        config.base_font_size,
    ));
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // (El resto de la hoja de estilos vive en `CSS_TEMPLATE`, al final del
    // archivo, para poder regenerarla al cambiar acento o tipografía.)

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let scroller = ScrolledWindow::new();
    // External = permite desplazamiento sin mostrar barra; así el contenido
    // ancho no obliga a la ventana a crecer.
    scroller.set_policy(PolicyType::External, PolicyType::Never);
    // El contenido de la marquesina no debe dictar el tamaño natural de la ventana.
    scroller.set_propagate_natural_width(false);
    scroller.set_propagate_natural_height(false);
    // Guardia extra: el scroller nunca debe pedir más que el espacio disponible.
    scroller.set_size_request(1, 1);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_halign(gtk4::Align::Fill);
    scroller.set_valign(gtk4::Align::Fill);

    let marquee = GtkBox::new(Orientation::Horizontal, 0);
    marquee.set_valign(gtk4::Align::Fill);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    // Arranque fuera de pantalla por la derecha (se recalcula por línea).
    spacer.set_width_request(800);
    marquee.append(&spacer);

    // Una palabra por `Label`: así cada una puede tener su propio tamaño y su
    // propio glow (Pango no permite sombras distintas dentro de un mismo label).
    let line_box = GtkBox::new(Orientation::Horizontal, 0);
    line_box.set_halign(gtk4::Align::Start);
    line_box.set_valign(gtk4::Align::Center);
    line_box.set_hexpand(false);
    line_box.set_vexpand(false);
    line_box.set_spacing(((config.base_font_size.max(6) * 4 / 3) / 3).max(4));
    marquee.append(&line_box);
    // Colchón final: permite que el texto salga completamente por la izquierda.
    let spacer_end = GtkBox::new(Orientation::Horizontal, 0);
    spacer_end.set_width_request(800);
    marquee.append(&spacer_end);
    scroller.set_child(Some(&marquee));
    root.append(&scroller);

    let progress = ProgressBar::new();
    progress.set_show_text(false);
    progress.set_visible(true);
    root.append(&progress);

    let controls = GtkBox::new(Orientation::Horizontal, 10);
    controls.set_halign(gtk4::Align::Center);
    controls.set_margin_start(18);
    controls.set_margin_end(18);
    controls.set_margin_bottom(14);
    controls.add_css_class("glass-bar");

    let nav = GtkBox::new(Orientation::Horizontal, 6);
    nav.set_valign(gtk4::Align::Center);

    let prev_file = Button::from_icon_name("media-skip-backward-symbolic");
    prev_file.set_tooltip_text(Some("Archivo anterior"));
    let prev_line = Button::from_icon_name("go-previous-symbolic");
    prev_line.set_tooltip_text(Some("Línea anterior (←)"));
    let play = Button::from_icon_name("media-playback-pause-symbolic");
    play.set_tooltip_text(Some("Pausa / reanudar (Espacio)"));
    let next_line = Button::from_icon_name("go-next-symbolic");
    next_line.set_tooltip_text(Some("Línea siguiente (→)"));
    let next_file = Button::from_icon_name("media-skip-forward-symbolic");
    next_file.set_tooltip_text(Some("Archivo siguiente"));
    for b in [&prev_file, &prev_line, &play, &next_line, &next_file] {
        b.add_css_class("control-btn");
        nav.append(b);
    }
    controls.append(&nav);

    let speed_scale = gtk4::Scale::with_range(Orientation::Horizontal, MIN_SPEED, MAX_SPEED, 0.05);
    speed_scale.set_value(config.speed.clamp(MIN_SPEED, MAX_SPEED));
    speed_scale.set_draw_value(false);
    speed_scale.set_size_request(120, -1);
    speed_scale.set_valign(gtk4::Align::Center);
    speed_scale.add_css_class("speed-scale");
    speed_scale.set_tooltip_text(Some("Velocidad de la marquesina (arrastra)"));
    let speed_value = Label::new(Some(&format!("{:.2}×", config.speed)));
    speed_value.add_css_class("speed-value");
    speed_value.set_width_chars(5);
    speed_value.set_valign(gtk4::Align::Center);
    let speed_box = GtkBox::new(Orientation::Horizontal, 6);
    speed_box.set_valign(gtk4::Align::Center);
    speed_box.append(&speed_scale);
    speed_box.append(&speed_value);
    controls.append(&speed_box);

    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.set_valign(gtk4::Align::Center);
    let fs_btn = Button::from_icon_name("view-fullscreen-symbolic");
    fs_btn.set_tooltip_text(Some("Pantalla completa (F11)"));
    let theme_btn = Button::from_icon_name("weather-clear-symbolic");
    theme_btn.set_tooltip_text(Some("Cambiar a modo claro"));
    let prefs_btn = Button::from_icon_name("preferences-system-symbolic");
    prefs_btn.set_tooltip_text(Some("Preferencias"));
    let open_btn = gtk4::MenuButton::new();
    open_btn.set_icon_name("folder-open-symbolic");
    open_btn.set_tooltip_text(Some("Abrir carpeta o archivo"));
    open_btn.add_css_class("control-btn");
    let open_menu = gtk4::Popover::new();
    let open_menu_box = GtkBox::new(Orientation::Vertical, 4);
    open_menu_box.set_margin_top(6);
    open_menu_box.set_margin_bottom(6);
    open_menu_box.set_margin_start(6);
    open_menu_box.set_margin_end(6);
    let open_dir_btn = Button::with_label("Abrir carpeta…");
    let open_file_btn = Button::with_label("Abrir archivo…");
    open_menu_box.append(&open_dir_btn);
    open_menu_box.append(&open_file_btn);
    open_menu.set_child(Some(&open_menu_box));
    open_btn.set_popover(Some(&open_menu));
    actions.append(&open_btn);
    for b in [&fs_btn, &theme_btn, &prefs_btn] {
        b.add_css_class("control-btn");
        actions.append(b);
    }
    controls.append(&actions);
    root.append(&controls);

    window.set_child(Some(&root));

    let events_queue = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ui = Rc::new(RefCell::new(UiState {
        config: config_rc.clone(),
        store: store.clone(),
        player: player.clone(),
        line_box: line_box.clone(),
        scroller: scroller.clone(),
        spacer: spacer.clone(),
        spacer_end: spacer_end.clone(),
        progress: progress.clone(),
        controls: controls.clone(),
        play_btn: play.clone(),
        fs_btn: fs_btn.clone(),
        theme_btn: theme_btn.clone(),
        speed_scale: speed_scale.clone(),
        speed_value: speed_value.clone(),
        window: window.clone(),
        cancel: cancel.clone(),
        root_tx: root_tx.clone(),
        top_n: config.top_n,
        base_font: config.base_font_size,
        min_font: config.min_font_size,
        max_font: config.max_font_size,
        font_family: config.font_family.clone(),
        accent_color: config.accent_color.clone(),
        line_gap_ms: config.line_gap_ms,
        css: css.clone(),
        events_queue: events_queue.clone(),
        scroll_source: RefCell::new(None),
        pending_scroll: RefCell::new(None),
        anim_elapsed: RefCell::new(0.0),
        last_tick: RefCell::new(std::time::Instant::now()),
        anim_from: RefCell::new(0.0),
        anim_to: RefCell::new(0.0),
        anim_duration: RefCell::new(1000),
        in_gap: RefCell::new(false),
        anim_line: RefCell::new(String::new()),
    }));

    // Tema: aplicar clases CSS y reaccionar si el sistema cambia en "auto".
    sync_theme_classes(&ui.borrow());
    sync_play_button(&ui.borrow());
    sync_fs_button(&ui.borrow());
    sync_speed_controls(&ui.borrow());
    {
        let ui_theme = ui.clone();
        libadwaita::StyleManager::default().connect_dark_notify(move |_| {
            sync_theme_classes(&ui_theme.borrow());
            render_current(&ui_theme);
        });
    }
    {
        let ui_fs = ui.clone();
        window.connect_fullscreened_notify(move |_| {
            sync_fs_button(&ui_fs.borrow());
        });
    }

    // keyboard
    let key_ctrl = gtk4::EventControllerKey::new();
    let ui_k = ui.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, state| {
        use gtk4::gdk::{Key, ModifierType};
        match key {
            Key::space => {
                let st = ui_k.borrow_mut();
                let mut p = st.player.borrow_mut();
                p.paused = !p.paused;
                drop(p);
                sync_play_button(&st);
                drop(st);
                glib::Propagation::Stop
            }
            Key::Left => {
                prev_line_action(&ui_k);
                glib::Propagation::Stop
            }
            Key::Right => {
                next_line_action(&ui_k);
                glib::Propagation::Stop
            }
            Key::F11 => {
                let st = ui_k.borrow();
                if st.window.is_fullscreen() {
                    st.window.unfullscreen();
                } else {
                    st.window.fullscreen();
                }
                glib::Propagation::Stop
            }
            Key::w | Key::W => {
                show_word_cloud(&ui_k);
                glib::Propagation::Stop
            }
            Key::q | Key::Q if state.contains(ModifierType::CONTROL_MASK) => {
                let window = ui_k.borrow().window.clone();
                window.close();
                glib::Propagation::Stop
            }
            Key::Escape => {
                let window = ui_k.borrow().window.clone();
                if window.is_fullscreen() {
                    window.unfullscreen();
                } else {
                    window.close();
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(key_ctrl);

    // mouse hide controls
    let motion = gtk4::EventControllerMotion::new();
    let ui_m = ui.clone();
    motion.connect_motion(move |_, _, _| {
        let st = ui_m.borrow_mut();
        st.controls.set_visible(true);
        st.player.borrow_mut().showing_controls = true;
        // reset hide timer by storing? We'll use a simple timeout via glib
    });
    window.add_controller(motion);

    // button signals
    let ui_b = ui.clone();
    play.connect_clicked(move |_| {
        let st = ui_b.borrow_mut();
        let mut p = st.player.borrow_mut();
        p.paused = !p.paused;
        drop(p);
        sync_play_button(&st);
    });
    let ui_b = ui.clone();
    next_line.connect_clicked(move |_| next_line_action(&ui_b));
    let ui_b = ui.clone();
    prev_line.connect_clicked(move |_| prev_line_action(&ui_b));
    let ui_b = ui.clone();
    next_file.connect_clicked(move |_| next_file_action(&ui_b));
    let ui_b = ui.clone();
    prev_file.connect_clicked(move |_| prev_file_action(&ui_b));
    let ui_s = ui.clone();
    speed_scale.connect_value_changed(move |scale| {
        let value = scale.value().clamp(MIN_SPEED, MAX_SPEED);
        let st = ui_s.borrow();
        let old = st.player.borrow().speed;
        if (old - value).abs() < f64::EPSILON {
            return;
        }
        st.player.borrow_mut().speed = value;
        st.config.borrow_mut().speed = value;
        // Ajustar el tiempo restante de la línea en curso para que el cambio
        // de velocidad se note al instante y sin saltos.
        let dur = *st.anim_duration.borrow();
        if !*st.in_gap.borrow() && old > 0.0 && dur > 0 {
            let fraction = (*st.anim_elapsed.borrow() / dur as f64).clamp(0.0, 1.0);
            let new_dur = (dur as f64 * old / value).clamp(200.0, 600_000.0);
            *st.anim_duration.borrow_mut() = new_dur as u64;
            *st.anim_elapsed.borrow_mut() = fraction * new_dur;
        }
        st.speed_value.set_text(&format!("{value:.2}×"));
    });
    let ui_b = ui.clone();
    fs_btn.connect_clicked(move |_| {
        let st = ui_b.borrow();
        if st.window.is_fullscreen() {
            st.window.unfullscreen();
        } else {
            st.window.fullscreen();
        }
    });
    let ui_b = ui.clone();
    prefs_btn.connect_clicked(move |_| {
        show_prefs(&ui_b);
    });
    let ui_t = ui.clone();
    theme_btn.connect_clicked(move |_| {
        let next = if theme_is_dark() { "light" } else { "dark" };
        {
            let st = ui_t.borrow();
            let mut c = st.config.borrow_mut();
            c.theme = next.to_string();
            let _ = c.save();
        }
        apply_theme_preference(next);
        sync_theme_classes(&ui_t.borrow());
        render_current(&ui_t);
    });
    let ui_b = ui.clone();
    let window_b = window.clone();
    let menu_b = open_menu.clone();
    open_dir_btn.connect_clicked(move |_| {
        menu_b.popdown();
        let current = {
            let st = ui_b.borrow();
            let folder = st.config.borrow().folder.clone();
            folder
        };
        let ui_c = ui_b.clone();
        pick_folder(&window_b, current, move |path| {
            apply_folder(&ui_c, path);
        });
    });
    let ui_b = ui.clone();
    let window_b = window.clone();
    let menu_b = open_menu.clone();
    open_file_btn.connect_clicked(move |_| {
        menu_b.popdown();
        let current = {
            let st = ui_b.borrow();
            let folder = st.config.borrow().folder.clone();
            folder
        };
        let ui_c = ui_b.clone();
        pick_file(&window_b, current, move |path| {
            apply_folder(&ui_c, path);
        });
    });

    window.present();

    // hide controls after 3s initially
    let ui_h = ui.clone();
    glib::timeout_add_local(Duration::from_secs(3), move || {
        let st = ui_h.borrow_mut();
        st.controls.set_visible(false);
        st.player.borrow_mut().showing_controls = false;
        glib::ControlFlow::Break
    });

    // start playback attempt
    let ui_p = ui.clone();
    glib::idle_add_local(move || {
        try_start_playback(&ui_p);
        glib::ControlFlow::Break
    });

    // first render triggers scroll
    let ui_t = ui.clone();
    glib::idle_add_local_once(move || {
        render_current(&ui_t);
    });

    // bridge thread: async_channel -> mutex queue
    let rx_c = rx;
    let q = events_queue.clone();
    std::thread::spawn(move || {
        while let Ok(first) = rx_c.recv_blocking() {
            let mut batch = vec![first];
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_millis(100) {
                match rx_c.try_recv() {
                    Ok(ev) => batch.push(ev),
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
            if let Ok(mut guard) = q.lock() {
                guard.extend(batch);
                // Cota dura: si la UI se retrasa, descartar eventos antiguos en vez
                // de acumular memoria sin límite. El estado relevante es el último.
                if guard.len() > 4096 {
                    let excess = guard.len() - 2048;
                    guard.drain(0..excess);
                }
            }
        }
    });

    // poll queue from main thread
    let ui_poll = ui.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let queue = ui_poll.borrow().events_queue.clone();
        // Intercambiar el contenido en vez de clonarlo: no duplicar la cola
        // completa cada 100 ms.
        let batch = match queue.lock() {
            Ok(mut g) => std::mem::take(&mut *g),
            Err(_) => Vec::new(),
        };
        if !batch.is_empty() {
            handle_events(&ui_poll, batch);
        }
        glib::ControlFlow::Continue
    });
}

fn handle_events(ui: &Rc<RefCell<UiState>>, events: Vec<IndexEvent>) {
    let st = ui.borrow_mut();
    let mut need_render = false;
    let mut first_words = false;
    let mut folder_empty = false;
    // Si la marquesina ya está en marcha, re-renderizar la reiniciaría desde el
    // principio (el indexador emite top-words cada ~100 ms). En ese caso el
    // nuevo resaltado se aplica en la siguiente línea.
    let already_animating = st.scroll_source.borrow().is_some();
    let mut latest_progress: Option<(usize, usize)> = None;
    for ev in events {
        match ev {
            IndexEvent::FileIndexed { .. } => {
                // try start playback if none
            }
            IndexEvent::IndexProgress { done, total } => {
                latest_progress = Some((done, total));
            }
            IndexEvent::TopWordsUpdated { words } => {
                let mut p = st.player.borrow_mut();
                first_words |= p.top_words.is_empty();
                p.top_words = words;
                drop(p);
                need_render = true;
            }
            IndexEvent::Error { path, message } => {
                tracing::warn!("index error {}: {}", path, message);
            }
            IndexEvent::Finished => {
                st.progress.set_visible(false);
                // Si la carpeta no tiene notas, avisar en vez de dejar la
                // marquesina vacía sin explicación.
                let root = st.config.borrow().folder.clone().unwrap_or_default();
                if paths_under_root(&st.store, &root).is_empty() {
                    folder_empty = true;
                }
            }
        }
    }
    if let Some((d, t)) = latest_progress {
        if t > 0 {
            st.progress.set_fraction(d as f64 / t as f64);
            st.progress.set_visible(true);
        }
    }
    drop(st);
    // try start playback
    try_start_playback(ui);
    if folder_empty {
        let st = ui.borrow();
        if st.player.borrow().current_path.is_none() {
            set_plain_line(
                &st,
                "No se encontraron notas Markdown en esta carpeta",
                st.base_font,
            );
            st.anim_line.borrow_mut().clear();
            st.progress.set_visible(false);
            return;
        }
    }
    if need_render && (first_words || !already_animating) {
        render_current(ui);
    }
}

/// Rutas indexadas que viven dentro de la carpeta activa. La caché SQLite es
/// global, así que al cambiar de carpeta hay que acotar las consultas para no
/// reproducir notas de la carpeta anterior.
fn paths_under_root(store: &Store, root: &std::path::Path) -> Vec<String> {
    store
        .all_paths_sorted()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| std::path::Path::new(p).starts_with(root))
        .collect()
}

fn pick_random(paths: &[String], exclude: Option<&str>) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    for _ in 0..8 {
        let i = rand::random_range(0..paths.len());
        if Some(paths[i].as_str()) != exclude {
            return Some(paths[i].clone());
        }
    }
    Some(paths[0].clone())
}

fn try_start_playback(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow_mut();
    let has_path = st.player.borrow().current_path.is_some();
    if has_path {
        return;
    }
    // try load first file
    let store = st.store.clone();
    let root = st.config.borrow().folder.clone().unwrap_or_default();
    let paths = paths_under_root(&store, &root);
    if paths.is_empty() {
        return;
    }
    let path = if st.player.borrow().order == PlayOrder::Random {
        pick_random(&paths, None)
    } else {
        Some(paths[0].clone())
    };
    if let Some(p) = path {
        let include_code = st.player.borrow().include_code;
        let lines = player::read_lines(std::path::Path::new(&p), include_code).unwrap_or_default();
        let mut pl = st.player.borrow_mut();
        pl.current_path = Some(p);
        pl.current_index = 0;
        pl.lines = if lines.is_empty() { vec!["(vacío)".into()] } else { lines };
        pl.line_idx = 0;
        pl.phase = Phase::Playing;
    }
    drop(st);
    render_current(ui);
}

fn clear_line(st: &UiState) {
    while let Some(child) = st.line_box.first_child() {
        st.line_box.remove(&child);
    }
}

/// Crea una palabra con su tamaño y glow propios. El color siempre se hereda
/// del CSS: las destacadas se fuerzan a blanco en la hoja de estilos.
fn make_word_label(text: &str, size: i32, bold: bool, glow_tier: Option<u8>) -> Label {
    let label = Label::new(None);
    label.add_css_class("splash");
    if let Some(tier) = glow_tier {
        label.add_css_class(&format!("word-glow-{tier}"));
    }
    label.set_halign(gtk4::Align::Start);
    label.set_valign(gtk4::Align::Baseline);
    label.set_vexpand(false);
    label.set_wrap(false);
    label.set_single_line_mode(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::None);
    let weight = if bold { " weight=\"bold\"" } else { "" };
    label.set_markup(&format!(
        "<span size=\"{}\"{}>{}</span>",
        size.max(6) * gtk4::pango::SCALE,
        weight,
        glib::markup_escape_text(text)
    ));
    label
}

fn set_plain_line(st: &UiState, text: &str, size: i32) {
    clear_line(st);
    let label = make_word_label(text, size, true, None);
    st.line_box.append(&label);
    st.line_box.queue_resize();
}

fn render_line(st: &UiState, line: &str, top: &[(String, String, i64)]) {
    clear_line(st);

    let mut map: std::collections::HashMap<String, (i64, usize)> =
        std::collections::HashMap::new();
    for (i, (norm, _disp, count)) in top.iter().enumerate() {
        map.insert(norm.clone(), (*count, i));
    }
    let max_count = top.first().map(|x| x.2).unwrap_or(1).max(1) as f64;
    // El tamaño normal es fijo para toda la app. Las palabras destacadas
    // crecen desde ese mismo tamaño hasta `max_font`.
    let low = st.base_font.max(st.min_font).max(6);
    let high = st.max_font.max(low);

    // Agrupar palabras consecutivas con el mismo estilo en un solo `Label`
    // reduce drásticamente el número de widgets y de sombras a compositar en
    // cada fotograma, que era la principal causa del lag.
    struct Run {
        text: String,
        size: i32,
        bold: bool,
        glow: Option<u8>,
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut push_word = |text: &str, size: i32, glow: Option<u8>| {
        let bold = glow.is_some();
        if let Some(last) = runs.last_mut() {
            if last.size == size && last.glow == glow && last.bold == bold {
                last.text.push(' ');
                last.text.push_str(text);
                return;
            }
        }
        runs.push(Run {
            text: text.to_string(),
            size,
            bold,
            glow,
        });
    };

    let mut chars = 0usize;
    for token in line.split_whitespace() {
        // Cota de seguridad: una línea monstruosa (JSON/código sin saltos) no
        // debe generar un layout gigantesco ni una marquesina interminable.
        chars += token.chars().count() + 1;
        if chars > 600 {
            push_word("…", st.base_font, None);
            break;
        }
        let stripped: String = token.chars().filter(|c| c.is_alphanumeric()).collect();
        let norm = analysis::normalize_word(&stripped);
        let (size, glow) = match map.get(&norm) {
            Some((count, rank)) => {
                let freq = (*count as f64 / max_count).sqrt().clamp(0.0, 1.0);
                let size = low as f64 + (high - low) as f64 * freq;
                let tier = if *rank < 5 {
                    3
                } else if *rank < 15 {
                    2
                } else {
                    1
                };
                (size.round() as i32, Some(tier))
            }
            None => (st.base_font, None),
        };
        push_word(token, size, glow);
    }

    for run in runs {
        st.line_box
            .append(&make_word_label(&run.text, run.size, run.bold, run.glow));
    }
    st.line_box.queue_resize();
}

fn render_current(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow();
    let pl = st.player.borrow();
    let line_key = if pl.phase == Phase::Title {
        let name = pl
            .current_path
            .as_ref()
            .map(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_default();
        set_plain_line(&st, &name, st.base_font);
        "title".to_string()
    } else {
        let line = pl.lines.get(pl.line_idx).cloned().unwrap_or_default();
        render_line(&st, &line, &pl.top_words);
        line
    };
    st.anim_line.borrow_mut().clone_from(&line_key);
    drop(pl);
    // Cancelar cualquier arranque pendiente para no acumular timers si llegan
    // varios eventos de top de palabras seguidos.
    if let Some(src) = st.pending_scroll.borrow_mut().take() {
        src.remove();
    }
    drop(st);
    // start horizontal scroll after layout
    let ui_s = ui.clone();
    let src = glib::timeout_add_local_once(std::time::Duration::from_millis(16), move || {
        // El timer ya se está ejecutando: vaciar el hueco para que nadie intente
        // cancelar un SourceId caducado.
        ui_s.borrow().pending_scroll.borrow_mut().take();
        start_scroll(&ui_s);
    });
    ui.borrow().pending_scroll.borrow_mut().replace(src);
}

fn advance(ui: Rc<RefCell<UiState>>) {
    let st = ui.borrow_mut();
    let mut pl = st.player.borrow_mut();
    if pl.lines.is_empty() {
        pl.lines.push("(vacío)".into());
    }
    match pl.phase {
        Phase::Playing => {
            if pl.line_idx + 1 < pl.lines.len() {
                pl.line_idx += 1;
            } else {
                pl.phase = Phase::Title;
            }
        }
        Phase::Title => {
            // load next file
            let store = st.store.clone();
            let include_code = pl.include_code;
            let order = pl.order;
            let current_path = pl.current_path.clone();
            let current_index = pl.current_index;
            let root = st.config.borrow().folder.clone().unwrap_or_default();
            let paths = paths_under_root(&store, &root);
            let next_path = if paths.is_empty() {
                None
            } else if order == PlayOrder::Random {
                pick_random(&paths, current_path.as_deref())
            } else {
                let ni = (current_index + 1) % paths.len();
                pl.current_index = ni;
                Some(paths[ni].clone())
            };
            if let Some(p) = next_path {
                let lines = player::read_lines(std::path::Path::new(&p), include_code).unwrap_or_default();
                pl.current_path = Some(p);
                pl.lines = if lines.is_empty() { vec!["(vacío)".into()] } else { lines };
                pl.line_idx = 0;
                pl.phase = Phase::Playing;
            } else {
                pl.phase = Phase::Playing;
            }
        }
    }
    drop(pl);
    drop(st);
    render_current(&ui);
}

fn next_line_action(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow_mut();
    let mut pl = st.player.borrow_mut();
    if pl.line_idx + 1 < pl.lines.len() {
        pl.line_idx += 1;
    }
    drop(pl);
    drop(st);
    render_current(ui);
}

fn prev_line_action(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow_mut();
    let mut pl = st.player.borrow_mut();
    if pl.line_idx > 0 {
        pl.line_idx -= 1;
    }
    drop(pl);
    drop(st);
    render_current(ui);
}

fn next_file_action(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow_mut();
    let mut pl = st.player.borrow_mut();
    pl.line_idx = pl.lines.len().saturating_sub(1); // force end -> next tick? Instead set phase title
    pl.phase = Phase::Title;
    drop(pl);
    drop(st);
    render_current(ui);
}

fn prev_file_action(ui: &Rc<RefCell<UiState>>) {
    // simplified: go to previous index
    let st = ui.borrow_mut();
    let mut pl = st.player.borrow_mut();
    if pl.current_index > 0 {
        pl.current_index -= 1;
    }
    let store = st.store.clone();
    let include_code = pl.include_code;
    let idx = pl.current_index;
    let root = st.config.borrow().folder.clone().unwrap_or_default();
    let paths = paths_under_root(&store, &root);
    if let Some(p) = paths.get(idx).cloned() {
        let lines = player::read_lines(std::path::Path::new(&p), include_code).unwrap_or_default();
        pl.current_path = Some(p);
        pl.lines = if lines.is_empty() { vec!["(vacío)".into()] } else { lines };
        pl.line_idx = 0;
        pl.phase = Phase::Playing;
    }
    drop(pl);
    drop(st);
    render_current(ui);
}

fn show_word_cloud(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow();
    let pl = st.player.borrow();
    let top = pl.top_words.clone();
    drop(pl);
    let maxc = top.first().map(|x| x.2).unwrap_or(1).max(1) as f64;
    let low = st.base_font.max(st.min_font).max(6);
    let high = st.max_font.max(low);
    let mut markup = String::from("<span>");
    for (_norm, disp, count) in &top {
        let freq = (*count as f64 / maxc).sqrt().clamp(0.0, 1.0);
        let size = (low as f64 + (high - low) as f64 * freq * 0.75) * gtk4::pango::SCALE as f64;
        markup.push_str(&format!(
            "<span size=\"{}\" weight=\"bold\">{} </span>",
            size as i32,
            glib::markup_escape_text(disp)
        ));
    }
    markup.push_str("</span>");

    let win = gtk4::ApplicationWindow::builder()
        .application(st.window.application().as_ref().unwrap())
        .title("Nube de palabras")
        .default_width(600)
        .default_height(400)
        .build();
    win.add_css_class(if theme_is_dark() { "tm-dark" } else { "tm-light" });
    let label = gtk4::Label::new(None);
    label.add_css_class("splash");
    label.set_markup(&markup);
    label.set_wrap(true);
    label.set_halign(gtk4::Align::Center);
    label.set_valign(gtk4::Align::Center);
    win.set_child(Some(&label));
    win.present();
}

/// Geometría de la marquesina. El contenido es `[hueco V][texto L][hueco V]`,
/// por lo que el ajuste máximo (`total - V`) es exactamente `V + L`: el texto
/// entra por la derecha y termina completamente fuera por la izquierda.
///
/// Devuelve `(ancho_total_del_contenido, recorrido)`.
fn marquee_metrics(viewport_w: f64, label_w: f64) -> (f64, f64) {
    let v = viewport_w.max(1.0);
    let l = label_w.max(1.0);
    (2.0 * v + l, v + l)
}

/// Duración del recorrido a velocidad constante. Depende sólo de la distancia
/// y de la velocidad elegida, nunca de la cantidad de palabras de la línea.
fn scroll_duration_ms(distance: f64, speed: f64) -> u64 {
    let speed = speed.clamp(MIN_SPEED, MAX_SPEED);
    (distance.max(1.0) / (650.0 * speed) * 1000.0)
        .round()
        .clamp(200.0, 600_000.0) as u64
}

fn start_scroll(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow_mut();
    // cancel previous
    if let Some(id) = st.scroll_source.borrow_mut().take() {
        id.remove();
    }

    // El viewport se vuelve a medir en cada línea. Ya no hay bucle de
    // realimentación porque el ScrolledWindow usa política EXTERNAL y no
    // propaga el ancho del hijo a la ventana.
    let viewport_w = {
        let w = st.scroller.width();
        if w > 1 {
            w as f64
        } else {
            st.window.width().max(1) as f64
        }
    };
    let (_min, label_w, _b1, _b2) = st.line_box.measure(gtk4::Orientation::Horizontal, -1);
    let (total, distance) = marquee_metrics(viewport_w, label_w as f64);

    // Reparto: [hueco V][texto L][hueco V]. El desplazamiento máximo es V + L,
    // justo lo necesario para que el texto entre por la derecha y salga por la
    // izquierda (antes se detenía a mitad de pantalla).
    st.spacer.set_width_request(viewport_w.round() as i32);
    st.spacer_end.set_width_request(viewport_w.round() as i32);

    let adj = st.scroller.hadjustment();
    adj.set_lower(0.0);
    adj.set_upper(total);
    adj.set_page_size(viewport_w);
    adj.set_value(0.0);

    // Todas las líneas —y también el título del archivo— recorren la misma
    // distancia a la misma velocidad en píxeles por segundo. Antes se usaba
    // `max(tiempo_de_lectura, recorrido)`, lo que hacía que las líneas largas
    // se movieran más despacio que las cortas.
    let duration = scroll_duration_ms(distance, st.player.borrow().speed);

    *st.anim_from.borrow_mut() = 0.0;
    *st.anim_to.borrow_mut() = distance;
    *st.anim_duration.borrow_mut() = duration;
    *st.anim_elapsed.borrow_mut() = 0.0;
    *st.in_gap.borrow_mut() = false;
    *st.last_tick.borrow_mut() = std::time::Instant::now();

    let ui_c = ui.clone();
    // add_tick_callback se sincroniza con el frame clock (vsync), a diferencia
    // de un timeout fijo de 16 ms: así el desplazamiento no tirita cuando el
    // intervalo real del timer oscila.
    let source = st.window.add_tick_callback(move |_window, _clock| {
        // Acumular sólo el tiempo realmente transcurrido y nunca el tiempo en
        // pausa: al reanudar la marquesina continúa donde estaba.
        let (adj, elapsed, dur, from, to, paused, in_gap, gap_ms) = {
            let st = ui_c.borrow();
            let now = std::time::Instant::now();
            let paused = st.player.borrow().paused;
            {
                let mut last = st.last_tick.borrow_mut();
                let dt = now.saturating_duration_since(*last).as_secs_f64() * 1000.0;
                *last = now;
                if !paused {
                    *st.anim_elapsed.borrow_mut() += dt.min(100.0);
                }
            }
            let adj = st.scroller.hadjustment();
            let elapsed = *st.anim_elapsed.borrow();
            let dur = *st.anim_duration.borrow();
            let from = *st.anim_from.borrow();
            let to = *st.anim_to.borrow();
            let in_gap = *st.in_gap.borrow();
            let gap_ms = st.line_gap_ms as f64;
            (adj, elapsed, dur, from, to, paused, in_gap, gap_ms)
        };

        if paused {
            return glib::ControlFlow::Continue;
        }

        // Hueco entre líneas: el texto ya salió por la izquierda, así que basta
        // con mantener el desplazamiento final durante la pausa configurada.
        if in_gap {
            if elapsed < gap_ms {
                return glib::ControlFlow::Continue;
            }
            {
                let st = ui_c.borrow();
                *st.scroll_source.borrow_mut() = None;
                *st.in_gap.borrow_mut() = false;
            }
            adj.set_value(to);
            advance(ui_c.clone());
            return glib::ControlFlow::Break;
        }

        if elapsed >= dur as f64 {
            if gap_ms > 0.0 {
                // Fin del recorrido: entrar en la pausa y reutilizar el mismo
                // contador de tiempo para no crear otro timer.
                {
                    let st = ui_c.borrow();
                    *st.in_gap.borrow_mut() = true;
                    *st.anim_elapsed.borrow_mut() = 0.0;
                    *st.last_tick.borrow_mut() = std::time::Instant::now();
                }
                adj.set_value(to);
                return glib::ControlFlow::Continue;
            }
            // Sin hueco configurado, pasar directamente a la línea siguiente.
            {
                let st = ui_c.borrow();
                *st.scroll_source.borrow_mut() = None;
            }
            adj.set_value(to);
            advance(ui_c.clone());
            return glib::ControlFlow::Break;
        }

        let t = elapsed / dur as f64;
        // Velocidad constante: una marquesina no debe acelerar ni frenar.
        adj.set_value(from + (to - from) * t);
        glib::ControlFlow::Continue
    });
    *st.scroll_source.borrow_mut() = Some(source);
}

fn pick_folder<W, F>(parent: &W, initial: Option<std::path::PathBuf>, on_pick: F)
where
    W: IsA<gtk4::Window>,
    F: FnOnce(std::path::PathBuf) + 'static,
{
    use gtk4::gio::prelude::*;

    let dialog = gtk4::FileDialog::builder()
        .title("Abrir carpeta de notas")
        .modal(true)
        .build();

    if let Some(dir) = initial.as_ref().filter(|p| p.is_dir()) {
        dialog.set_initial_folder(Some(&gtk4::gio::File::for_path(dir)));
    }

    let on_pick = Rc::new(RefCell::new(Some(on_pick)));
    dialog.select_folder(
        Some(parent),
        None::<&gtk4::gio::Cancellable>,
        move |result| match result {
            Ok(file) => {
                if let Some(path) = file.path() {
                    if let Some(cb) = on_pick.borrow_mut().take() {
                        cb(path);
                    }
                }
            }
            Err(err) => {
                // Cancelar el diálogo no es un error que deba molestar.
                if !err.matches(gtk4::gio::IOErrorEnum::Cancelled) {
                    tracing::warn!("no se pudo elegir carpeta: {err}");
                }
            }
        },
    );
}

fn pick_file<W, F>(parent: &W, initial: Option<std::path::PathBuf>, on_pick: F)
where
    W: IsA<gtk4::Window>,
    F: FnOnce(std::path::PathBuf) + 'static,
{
    use gtk4::gio::prelude::*;

    let dialog = gtk4::FileDialog::builder()
        .title("Abrir archivo Markdown")
        .modal(true)
        .build();

    let filter = gtk4::FileFilter::new();
    filter.set_name(Some("Notas Markdown"));
    filter.add_pattern("*.md");
    filter.add_pattern("*.markdown");
    filter.add_pattern("*.MD");
    filter.add_pattern("*.MARKDOWN");
    let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));

    if let Some(path) = initial {
        if path.is_file() {
            dialog.set_initial_file(Some(&gtk4::gio::File::for_path(&path)));
        } else if path.is_dir() {
            dialog.set_initial_folder(Some(&gtk4::gio::File::for_path(&path)));
        }
    }

    let on_pick = Rc::new(RefCell::new(Some(on_pick)));
    dialog.open(
        Some(parent),
        None::<&gtk4::gio::Cancellable>,
        move |result| match result {
            Ok(file) => {
                if let Some(path) = file.path() {
                    if let Some(cb) = on_pick.borrow_mut().take() {
                        cb(path);
                    }
                }
            }
            Err(err) => {
                if !err.matches(gtk4::gio::IOErrorEnum::Cancelled) {
                    tracing::warn!("no se pudo elegir archivo: {err}");
                }
            }
        },
    );
}

/// Una raíz válida es una carpeta o un archivo Markdown individual.
fn is_valid_root(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let is_md = ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown");
    path.is_dir() || (path.is_file() && is_md)
}

/// Cambia la carpeta activa: actualiza la configuración, limpia la caché y el
/// reproductor, y pide al supervisor que reindexe la carpeta nueva.
fn apply_folder(ui: &Rc<RefCell<UiState>>, path: std::path::PathBuf) {
    if !is_valid_root(&path) {
        let window = ui.borrow().window.clone();
        let alert = gtk4::AlertDialog::builder()
            .message("Carpeta o archivo no válido")
            .detail(format!(
                "«{}» no existe, no es una carpeta ni un archivo Markdown.",
                path.display()
            ))
            .build();
        alert.show(Some(&window));
        return;
    }

    let (store, root_tx, config) = {
        let st = ui.borrow();
        (st.store.clone(), st.root_tx.clone(), st.config.clone())
    };

    {
        let mut c = config.borrow_mut();
        c.folder = Some(path.clone());
        if let Err(err) = c.save() {
            tracing::warn!("no se pudo guardar la configuración: {err}");
        }
    }

    {
        let st = ui.borrow();
        let mut pl = st.player.borrow_mut();
        pl.current_path = None;
        pl.current_index = 0;
        pl.lines.clear();
        pl.line_idx = 0;
        pl.phase = Phase::Playing;
        pl.top_words.clear();
        st.progress.set_visible(false);
        *st.anim_elapsed.borrow_mut() = 0.0;
    }

    // Vaciar la caché evita que las notas de la carpeta anterior sigan
    // apareciendo mientras se indexa la nueva.
    if let Err(err) = store.clear() {
        tracing::warn!("no se pudo vaciar la caché: {err}");
    }

    if root_tx.send(path.clone()).is_err() {
        tracing::warn!("el indexador ya no está activo; reinicia la aplicación");
    }

    // La ventana no muestra título; sólo recargamos la línea con la carpeta nueva.
    render_current(ui);
}

fn show_prefs(ui: &Rc<RefCell<UiState>>) {
    let st = ui.borrow();
    let window = st.window.clone();
    let config_rc = st.config.clone();
    let player = st.player.clone();
    drop(st);

    let cfg = config_rc.borrow().clone();
    let pref = gtk4::Window::builder()
        .title("Preferencias")
        .modal(true)
        .transient_for(&window)
        .default_width(480)
        .default_height(620)
        .build();
    pref.add_css_class(if theme_is_dark() { "tm-dark" } else { "tm-light" });

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let folder_entry = gtk4::Entry::new();
    folder_entry.set_text(cfg.folder.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default().as_str());
    folder_entry.set_hexpand(true);
    let folder_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    folder_box.append(&gtk4::Label::new(Some("Carpeta o archivo:")));
    folder_box.append(&folder_entry);
    let choose_btn = gtk4::Button::with_label("Carpeta…");
    folder_box.append(&choose_btn);
    let file_btn = gtk4::Button::with_label("Archivo…");
    folder_box.append(&file_btn);
    vbox.append(&folder_box);
    {
        let entry_c = folder_entry.clone();
        let pref_c = pref.clone();
        choose_btn.connect_clicked(move |_| {
            let entry_cc = entry_c.clone();
            let initial = Some(std::path::PathBuf::from(entry_c.text().as_str()));
            pick_folder(&pref_c, initial, move |path| {
                entry_cc.set_text(&path.to_string_lossy());
            });
        });
    }
    {
        let entry_c = folder_entry.clone();
        let pref_c = pref.clone();
        file_btn.connect_clicked(move |_| {
            let entry_cc = entry_c.clone();
            let initial = Some(std::path::PathBuf::from(entry_c.text().as_str()));
            pick_file(&pref_c, initial, move |path| {
                entry_cc.set_text(&path.to_string_lossy());
            });
        });
    }

    let top_spin = gtk4::SpinButton::with_range(10.0, 200.0, 1.0);
    top_spin.set_value(cfg.top_n as f64);
    let top_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    top_box.append(&gtk4::Label::new(Some("Top N:")));
    top_box.append(&top_spin);
    vbox.append(&top_box);

    let speed_spin = gtk4::SpinButton::with_range(MIN_SPEED, MAX_SPEED, 0.05);
    speed_spin.set_value(cfg.speed.clamp(MIN_SPEED, MAX_SPEED));
    let speed_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    speed_box.append(&gtk4::Label::new(Some("Velocidad:")));
    speed_box.append(&speed_spin);
    vbox.append(&speed_box);

    let gap_spin = gtk4::SpinButton::with_range(0.0, 10_000.0, 50.0);
    gap_spin.set_value(cfg.line_gap_ms as f64);
    let gap_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    gap_box.append(&gtk4::Label::new(Some("Espacio entre líneas (ms):")));
    gap_box.append(&gap_spin);
    vbox.append(&gap_box);

    // Fuente instalada en el sistema (sólo familia; el tamaño se controla aparte).
    let font_btn = gtk4::FontDialogButton::new(Some(gtk4::FontDialog::new()));
    font_btn.set_level(gtk4::FontLevel::Family);
    font_btn.set_use_font(false);
    font_btn.set_use_size(false);
    let mut font_desc = gtk4::pango::FontDescription::new();
    font_desc.set_family(&cfg.font_family);
    font_btn.set_font_desc(&font_desc);
    font_btn.set_hexpand(true);
    let font_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    font_box.append(&gtk4::Label::new(Some("Fuente:")));
    font_box.append(&font_btn);
    vbox.append(&font_box);

    let base_spin = gtk4::SpinButton::with_range(8.0, 96.0, 1.0);
    base_spin.set_value(cfg.base_font_size as f64);
    let base_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    base_box.append(&gtk4::Label::new(Some("Tamaño normal:")));
    base_box.append(&base_spin);
    vbox.append(&base_box);

    let max_spin = gtk4::SpinButton::with_range(8.0, 200.0, 1.0);
    max_spin.set_value(cfg.max_font_size as f64);
    let max_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    max_box.append(&gtk4::Label::new(Some("Tamaño máximo destacado:")));
    max_box.append(&max_spin);
    vbox.append(&max_box);

    let accent_btn = gtk4::ColorDialogButton::new(Some(gtk4::ColorDialog::new()));
    accent_btn.set_rgba(
        &gtk4::gdk::RGBA::parse(&cfg.accent_color).unwrap_or(gtk4::gdk::RGBA::RED),
    );
    accent_btn.set_hexpand(true);
    let accent_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    accent_box.append(&gtk4::Label::new(Some("Color de acento:")));
    accent_box.append(&accent_btn);
    vbox.append(&accent_box);

    let string_list = gtk4::StringList::new(&["Alfabético", "Aleatorio"]);
    let order_combo = gtk4::DropDown::new(Some(string_list), None::<gtk4::Expression>);
    if cfg.order == "random" {
        order_combo.set_selected(1);
    } else {
        order_combo.set_selected(0);
    }
    let order_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    order_box.append(&gtk4::Label::new(Some("Orden:")));
    order_box.append(&order_combo);
    vbox.append(&order_box);

    let theme_list = gtk4::StringList::new(&["Automático", "Oscuro", "Claro"]);
    let theme_combo = gtk4::DropDown::new(Some(theme_list), None::<gtk4::Expression>);
    theme_combo.set_selected(match cfg.theme.as_str() {
        "dark" => 1,
        "light" => 2,
        _ => 0,
    });
    let theme_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    theme_box.append(&gtk4::Label::new(Some("Tema:")));
    theme_box.append(&theme_combo);
    vbox.append(&theme_box);

    let code_check = gtk4::CheckButton::with_label("Incluir bloques de código");
    code_check.set_active(cfg.include_code_blocks);
    vbox.append(&code_check);

    let save_btn = gtk4::Button::with_label("Guardar");
    save_btn.connect_clicked({
        let config_rc = config_rc.clone();
        let player = player.clone();
        let pref = pref.clone();
        let ui = ui.clone();
        let folder_entry = folder_entry.clone();
        let top_spin = top_spin.clone();
        let speed_spin = speed_spin.clone();
        let gap_spin = gap_spin.clone();
        let order_combo = order_combo.clone();
        let theme_combo = theme_combo.clone();
        let code_check = code_check.clone();
        let font_btn = font_btn.clone();
        let base_spin = base_spin.clone();
        let max_spin = max_spin.clone();
        let accent_btn = accent_btn.clone();
        move |_| {
            let new_folder = crate::config::expand_tilde(&folder_entry.text());
            let theme = match theme_combo.selected() {
                1 => "dark".to_string(),
                2 => "light".to_string(),
                _ => "auto".to_string(),
            };
            let folder_changed = {
                let c = config_rc.borrow();
                c.folder.as_ref() != Some(&new_folder)
            };

            if folder_changed && !is_valid_root(&new_folder) {
                let alert = gtk4::AlertDialog::builder()
                    .message("Carpeta o archivo no válido")
                    .detail(format!(
                        "«{}» no existe, no es una carpeta ni un archivo Markdown.",
                        new_folder.display()
                    ))
                    .build();
                alert.show(Some(&pref));
                return;
            }

            let selected_font = font_btn
                .font_desc()
                .and_then(|d| d.family().map(|f| f.to_string()))
                .filter(|f| !f.trim().is_empty());
            let accent_color = rgba_to_hex(&accent_btn.rgba());
            let base_size = base_spin.value().round().clamp(8.0, 96.0) as i32;
            let max_size = max_spin.value().round().clamp(8.0, 200.0) as i32;

            {
                let mut c = config_rc.borrow_mut();
                c.folder = Some(new_folder.clone());
                c.top_n = top_spin.value() as usize;
                c.speed = speed_spin.value().clamp(MIN_SPEED, MAX_SPEED);
                c.line_gap_ms = gap_spin.value().round().clamp(0.0, 10_000.0) as u64;
                c.order = if order_combo.selected() == 1 { "random".into() } else { "alphabetical".into() };
                c.include_code_blocks = code_check.is_active();
                c.theme = theme.clone();
                c.base_font_size = base_size;
                c.max_font_size = max_size;
                if let Some(font) = selected_font {
                    c.font_family = font;
                }
                c.accent_color = accent_color;
                let _ = c.save();

                let mut p = player.borrow_mut();
                p.speed = c.speed;
                p.include_code = c.include_code_blocks;
                p.order = if c.order == "random" { crate::player::PlayOrder::Random } else { crate::player::PlayOrder::Alphabetical };
            }

            // Recrear la hoja de estilos para aplicar acento, fuente y tamaño.
            {
                let mut st = ui.borrow_mut();
                st.base_font = base_size;
                st.max_font = max_size;
                st.font_family = config_rc.borrow().font_family.clone();
                st.accent_color = config_rc.borrow().accent_color.clone();
                st.line_gap_ms = config_rc.borrow().line_gap_ms;
                st.line_box
                    .set_spacing(((base_size.max(6) * 4 / 3) / 3).max(4));
                reload_css(&st);
            }

            sync_speed_controls(&ui.borrow());
            apply_theme_preference(&theme);
            sync_theme_classes(&ui.borrow());
            render_current(&ui);

            if folder_changed {
                // Reindexar y limpiar la caché de la carpeta anterior.
                apply_folder(&ui, new_folder);
            }

            pref.close();
        }
    });
    vbox.append(&save_btn);

    let about_btn = gtk4::Button::with_label("Acerca de");
    about_btn.connect_clicked(move |_| {
        let dialog = gtk4::AboutDialog::builder()
            .program_name("Mr. Reader")
            .version("0.1.0")
            .comments("Splash de notas Markdown")
            .license_type(gtk4::License::Gpl30)
            .build();
        dialog.present();
    });
    vbox.append(&about_btn);

    pref.set_child(Some(&vbox));
    pref.present();
}


const CSS_TEMPLATE: &str = r#"
        __COLORS__

        /* ── Material Aero · oscuro ─────────────────────────────── */
        window.tm-dark {
            background-color: @bg_dark;
            background-image: radial-gradient(circle at 50% 34%, @bg_dark_center 0%, @bg_dark 60%, @bg_dark_deep 100%);
        }
        window.tm-dark headerbar.tm-titlebar {
            background: transparent;
            background-color: transparent;
            background-image: none;
            border: none;
            box-shadow: none;
            min-height: 30px;
        }
        label.splash {
            color: #ffffff;
            font-family: __FONT_FAMILY__;
            font-size: __BASE_FONT__pt;
            font-weight: 600;
            letter-spacing: 1px;
        }
        window.tm-dark label.splash {
            color: #ffffff;
            text-shadow:
                -2px -2px 0 @outline_dark, 0 -2px 0 @outline_dark, 2px -2px 0 @outline_dark,
                -2px 0 0 @outline_dark, 2px 0 0 @outline_dark,
                -2px 2px 0 @outline_dark, 0 2px 0 @outline_dark, 2px 2px 0 @outline_dark,
                0 0 9px @glow_a,
                0 0 22px @glow_b;
        }
        window.tm-dark progressbar trough {
            min-height: 3px;
            background-color: rgba(255, 255, 255, 0.08);
        }
        window.tm-dark progressbar progress {
            background-image: linear-gradient(90deg, @accent_soft, @accent);
        }
        window.tm-dark button {
            background-image: linear-gradient(180deg, rgba(255,255,255,0.10), rgba(255,255,255,0.03));
            background-color: rgba(255,255,255,0.04);
            color: #ffe9ee;
            border: 1px solid rgba(255,255,255,0.12);
            border-radius: 9px;
        }
        window.tm-dark .glass-bar {
            background-image: linear-gradient(180deg, rgba(255,255,255,0.10), rgba(255,255,255,0.03));
            background-color: @glass_dark;
            border: 1px solid @accent_a30;
            border-radius: 22px;
            padding: 6px 12px;
            box-shadow: 0 8px 26px rgba(0, 0, 0, 0.38), inset 0 1px 0 rgba(255, 255, 255, 0.14);
        }
        window.tm-dark .control-btn {
            min-width: 34px;
            min-height: 34px;
            padding: 0;
            border-radius: 17px;
            background-image: linear-gradient(180deg, rgba(255,255,255,0.12), rgba(255,255,255,0.03));
            background-color: rgba(255,255,255,0.04);
            color: #ffe9ee;
            border: 1px solid rgba(255,255,255,0.12);
            box-shadow: 0 2px 8px rgba(0,0,0,0.30), inset 0 1px 0 rgba(255,255,255,0.12);
        }
        window.tm-dark .control-btn:hover {
            background-image: linear-gradient(180deg, @accent_soft, @accent);
            background-color: @accent;
            color: #ffffff;
            border-color: @accent_a65;
            box-shadow: 0 0 16px @accent_a55, inset 0 1px 0 rgba(255,255,255,0.25);
        }
        window.tm-dark .control-btn:active {
            background-image: linear-gradient(180deg, @accent, @accent_deep);
        }
        window.tm-dark .speed-scale trough {
            min-height: 6px;
            border-radius: 3px;
            background-color: rgba(255, 255, 255, 0.16);
        }
        window.tm-dark .speed-scale highlight {
            background-image: linear-gradient(90deg, @accent_soft, @accent);
            border-radius: 3px;
        }
        window.tm-dark .speed-scale slider {
            min-width: 16px;
            min-height: 16px;
            margin: -5px 0;
            border-radius: 8px;
            background-color: #ffffff;
            border: 1px solid @accent_a55;
            box-shadow: 0 0 10px @accent_a65;
        }
        .speed-value {
            color: @accent_soft;
            font-family: __FONT_FAMILY__;
            font-size: 11pt;
            font-weight: 600;
        }

        /* ── Material Aero · claro ──────────────────────────────── */
        window.tm-light {
            background-color: @bg_light;
            background-image: radial-gradient(circle at 50% 34%, @bg_light_center 0%, @bg_light 55%, @bg_light_deep 100%);
        }
        window.tm-light headerbar.tm-titlebar {
            background: transparent;
            background-color: transparent;
            background-image: none;
            border: none;
            box-shadow: none;
            min-height: 30px;
        }
        window.tm-light label.splash {
            color: #1b0d11;
            text-shadow:
                -2px -2px 0 @outline_light, 0 -2px 0 @outline_light, 2px -2px 0 @outline_light,
                -2px 0 0 @outline_light, 2px 0 0 @outline_light,
                -2px 2px 0 @outline_light, 0 2px 0 @outline_light, 2px 2px 0 @outline_light,
                0 0 8px @glow_a,
                0 0 20px @glow_b;
        }
        window.tm-light progressbar trough {
            min-height: 3px;
            background-color: @accent_a12;
        }
        window.tm-light progressbar progress {
            background-image: linear-gradient(90deg, @accent, @accent_deep);
        }
        window.tm-light button {
            background-image: linear-gradient(180deg, #ffffff, @bg_light);
            background-color: #ffffff;
            color: @accent_deep;
            border: 1px solid @accent_a25;
            border-radius: 9px;
        }
        window.tm-light .glass-bar {
            background-image: linear-gradient(180deg, rgba(255,255,255,0.90), rgba(255,255,255,0.45));
            background-color: @glass_light;
            border: 1px solid @accent_a25;
            border-radius: 22px;
            padding: 6px 12px;
            box-shadow: 0 8px 24px @accent_a18, inset 0 1px 0 #ffffff;
        }
        window.tm-light .control-btn {
            min-width: 34px;
            min-height: 34px;
            padding: 0;
            border-radius: 17px;
            background-image: linear-gradient(180deg, #ffffff, @bg_light);
            background-color: #ffffff;
            color: @accent_deep;
            border: 1px solid @accent_a25;
            box-shadow: 0 2px 8px @accent_a12, inset 0 1px 0 #ffffff;
        }
        window.tm-light .control-btn:hover {
            background-image: linear-gradient(180deg, @accent_soft, @accent);
            background-color: @accent;
            color: #ffffff;
            border-color: @accent_a65;
            box-shadow: 0 0 16px @accent_a45, inset 0 1px 0 rgba(255,255,255,0.35);
        }
        window.tm-light .control-btn:active {
            background-image: linear-gradient(180deg, @accent, @accent_deep);
        }
        window.tm-light .speed-scale trough {
            min-height: 6px;
            border-radius: 3px;
            background-color: @accent_a18;
        }
        window.tm-light .speed-scale highlight {
            background-image: linear-gradient(90deg, @accent, @accent_deep);
            border-radius: 3px;
        }
        window.tm-light .speed-scale slider {
            min-width: 16px;
            min-height: 16px;
            margin: -5px 0;
            border-radius: 8px;
            background-color: #ffffff;
            border: 1px solid @accent_a55;
            box-shadow: 0 0 10px @accent_a45;
        }
        window.tm-light .speed-value {
            color: @accent_deep;
        }

        /* ── Palabras destacadas: siempre blancas, con glow creciente ── */
        window.tm-dark label.splash.word-glow-1,
        window.tm-light label.splash.word-glow-1 {
            color: #ffffff;
            text-shadow:
                -2px -2px 0 @outline_dark, 0 -2px 0 @outline_dark, 2px -2px 0 @outline_dark,
                -2px 0 0 @outline_dark, 2px 0 0 @outline_dark,
                -2px 2px 0 @outline_dark, 0 2px 0 @outline_dark, 2px 2px 0 @outline_dark,
                0 0 12px @glow_a,
                0 0 28px @glow_b;
        }
        window.tm-dark label.splash.word-glow-2,
        window.tm-light label.splash.word-glow-2 {
            color: #ffffff;
            text-shadow:
                -2px -2px 0 @outline_dark, 0 -2px 0 @outline_dark, 2px -2px 0 @outline_dark,
                -2px 0 0 @outline_dark, 2px 0 0 @outline_dark,
                -2px 2px 0 @outline_dark, 0 2px 0 @outline_dark, 2px 2px 0 @outline_dark,
                0 0 16px @glow_hi_a,
                0 0 34px @glow_hi_b;
        }
        window.tm-dark label.splash.word-glow-3,
        window.tm-light label.splash.word-glow-3 {
            color: #ffffff;
            text-shadow:
                -2px -2px 0 @outline_dark, 0 -2px 0 @outline_dark, 2px -2px 0 @outline_dark,
                -2px 0 0 @outline_dark, 2px 0 0 @outline_dark,
                -2px 2px 0 @outline_dark, 0 2px 0 @outline_dark, 2px 2px 0 @outline_dark,
                0 0 20px @glow_hi_a,
                0 0 44px @glow_hi_b;
        }
        "#;

#[cfg(test)]
mod marquee_tests {
    use super::{
        build_css, hex_color, marquee_metrics, mix_color, parse_hex_color, rgba_color,
        scroll_duration_ms,
    };

    #[test]
    fn css_usa_acento_fuente_y_tamano() {
        let css = build_css("#00aaff", "DejaVu Sans", 28);
        assert!(css.contains("@define-color accent #00aaff;"));
        assert!(css.contains("\"DejaVu Sans\""));
        assert!(css.contains("font-size: 28pt;"));
        assert!(!css.contains("__COLORS__"));
        assert!(!css.contains("__FONT_FAMILY__"));
        assert!(!css.contains("__BASE_FONT__"));
    }

    #[test]
    fn color_acento_invalido_cae_al_rojo_por_defecto() {
        let css = build_css("no-es-un-color", "monospace", 22);
        assert!(css.contains("@define-color accent #e5304a;"));
    }

    #[test]
    fn parsea_y_mezcla_colores() {
        assert_eq!(parse_hex_color("#e5304a"), Some((0xe5, 0x30, 0x4a)));
        assert_eq!(parse_hex_color("e5304a"), Some((0xe5, 0x30, 0x4a)));
        assert_eq!(parse_hex_color("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_color("ñ"), None);
        assert_eq!(mix_color((0, 0, 0), (255, 255, 255), 0.5), (128, 128, 128));
        assert_eq!(hex_color((1, 2, 3)), "#010203");
        assert_eq!(rgba_color((1, 2, 3), 0.5), "rgba(1, 2, 3, 0.50)");
    }

    #[test]
    fn recorrido_cubre_viewport_y_texto() {
        let v = 800.0;
        let l = 3000.0;
        let (total, travel) = marquee_metrics(v, l);
        // [V][L][V]
        assert_eq!(total, 2.0 * v + l);
        // Al llegar al final, el borde derecho del texto queda en x = 0.
        assert_eq!(travel, total - v);
        assert_eq!(travel, v + l);
    }

    #[test]
    fn geometria_estable_ante_valores_nulos() {
        let (total, travel) = marquee_metrics(0.0, 0.0);
        assert!(total.is_finite() && total > 0.0);
        assert!(travel.is_finite() && travel > 0.0);
        assert_eq!(total - travel, 1.0);
    }

    #[test]
    fn recorrido_crece_con_la_ventana_sin_multiplicarse() {
        let (t1, d1) = marquee_metrics(800.0, 1200.0);
        let (t2, d2) = marquee_metrics(1600.0, 1200.0);
        assert_eq!(t2 - t1, 1600.0);
        assert_eq!(d2 - d1, 800.0);
    }

    #[test]
    fn velocidad_es_constante_entre_lineas() {
        // La relación px/ms es la misma para una línea corta y una larga; sólo
        // cambia la duración total en proporción a la distancia.
        let short = scroll_duration_ms(1000.0, 1.0);
        let long = scroll_duration_ms(4000.0, 1.0);
        assert_eq!(short, 1538);
        assert_eq!(long, 6154);
        assert!((long as f64 / short as f64 - 4.0).abs() < 0.01);
    }

    #[test]
    fn velocidad_escala_con_el_deslizador() {
        let normal = scroll_duration_ms(2000.0, 1.0);
        let fast = scroll_duration_ms(2000.0, 2.0);
        assert!(fast < normal);
        assert!((normal as f64 / fast as f64 - 2.0).abs() < 0.01);
        // El mínimo evita duraciones de 0 ms en líneas muy cortas.
        assert_eq!(scroll_duration_ms(1.0, 4.0), 200);
    }
}
