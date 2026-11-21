use crate::{
    decode::{self, Picture},
    metadata::{self, Tag},
    process::Result,
    shell,
    view::View,
};
use eframe::egui::{
    self, Align2, Area, Button, Color32, ColorImage, Context, FontId, Frame, Id, Key, Pos2, Rect,
    RichText, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2, ViewportCommand, pos2, vec2,
};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const TILE: u32 = 1024;
const PREVIEW_SIDE: u32 = 3072;
const TITLE: f32 = 44.0;
const STATUS: f32 = 30.0;

#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "авто",
            Self::Light => "светлая",
            Self::Dark => "тёмная",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Auto => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::Auto,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Settings {
    theme: Theme,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
    pub context_menu: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Auto,
            width: 1440.0,
            height: 900.0,
            maximized: false,
            context_menu: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let read = |path| {
            std::fs::read(path)
                .ok()
                .and_then(|data| serde_json::from_slice(&data).ok())
        };
        settings_path()
            .and_then(read)
            .or_else(|| {
                let base = std::env::var_os("LOCALAPPDATA")?;
                read(PathBuf::from(base).join("ThodView/settings.json"))
            })
            .unwrap_or_default()
    }

    fn save(&self) -> Result<()> {
        let Some(path) = settings_path() else {
            return Ok(());
        };
        let folder = path.parent().unwrap();
        std::fs::create_dir_all(folder)?;
        let mut file = tempfile::NamedTempFile::new_in(folder)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.persist(path)?;
        Ok(())
    }
}

fn settings_path() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("ThodHUB/Data/thodview/settings.json"),
    )
}

#[derive(Clone, Copy)]
struct Palette {
    canvas: Color32,
    panel: Color32,
    text: Color32,
    muted: Color32,
    line: Color32,
    hover: Color32,
    accent: Color32,
}

impl Palette {
    fn new(light: bool) -> Self {
        let color = |rgb: u32| Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        let [canvas, panel, text, muted, line, hover, accent] = if light {
            [
                0xe8ecf3, 0xffffff, 0x131820, 0x5f6a7c, 0xdce2ec, 0xedf1f7, 0x2f6fed,
            ]
        } else {
            [
                0x0b0d11, 0x151920, 0xedf1f7, 0x8b94a6, 0x242a35, 0x1d2330, 0x5b8cff,
            ]
        }
        .map(color);
        Self {
            canvas,
            panel,
            text,
            muted,
            line,
            hover,
            accent,
        }
    }
}

fn icon(ch: char, size: f32) -> RichText {
    RichText::new(ch.to_string()).font(FontId::new(
        size,
        egui::FontFamily::Name("windows-icons".into()),
    ))
}

fn job<T: Send + 'static>(
    ctx: &Context,
    action: impl FnOnce() -> Result<T> + Send + 'static,
) -> Receiver<Result<T>> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .unwrap_or_else(|_| Err("Операция прервана из-за ошибки декодера".into()));
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    rx
}

struct Loaded {
    path: PathBuf,
    folder: Vec<PathBuf>,
    picture: Picture,
    page: u32,
    preview: Option<RgbaImage>,
}

fn preview(image: &RgbaImage) -> Option<RgbaImage> {
    if image.width().max(image.height()) <= PREVIEW_SIDE {
        return None;
    }
    let ratio = PREVIEW_SIDE as f64 / image.width().max(image.height()) as f64;
    Some(image::imageops::resize(
        image,
        (image.width() as f64 * ratio).round().max(1.0) as u32,
        (image.height() as f64 * ratio).round().max(1.0) as u32,
        image::imageops::FilterType::Triangle,
    ))
}

fn sibling_files(path: &Path) -> Vec<PathBuf> {
    let mut files = path
        .parent()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let ty = entry.file_type().ok()?;
            if !ty.is_dir() {
                let file = entry.path();
                if decode::is_image(&file) {
                    return Some(file);
                }
            }
            None
        })
        .collect::<Vec<_>>();
    if !files.iter().any(|item| item == path) {
        files.push(path.to_owned());
    }
    files.sort_by_cached_key(|file| {
        file.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    files
}

#[derive(Default)]
struct Textures {
    overview: Option<TextureHandle>,
    tiles: HashMap<(u32, u32), TextureHandle>,
}

impl Textures {
    fn set(&mut self, ctx: &Context, image: &RgbaImage, preview: Option<&RgbaImage>) {
        self.tiles.clear();
        let pixels = preview.unwrap_or(image);
        let maximum = ctx.input(|i| i.max_texture_side);
        self.overview = if (pixels.width().max(pixels.height()) as usize) <= maximum {
            Some(ctx.load_texture(
                "Изображение",
                color_image(pixels),
                TextureOptions {
                    magnification: egui::TextureFilter::Nearest,
                    ..TextureOptions::LINEAR
                },
            ))
        } else {
            None
        };
    }

    fn paint(&mut self, ui: &Ui, rect: Rect, canvas: Rect, image: &RgbaImage, scale: f32) {
        let painter = ui.painter().with_clip_rect(canvas);
        let use_tiles = self.overview.is_none()
            || (image.width().max(image.height()) > PREVIEW_SIDE
                && scale * ui.ctx().pixels_per_point()
                    > PREVIEW_SIDE as f32 / image.width().max(image.height()) as f32);
        if !use_tiles {
            if let Some(overview) = &self.overview {
                painter.image(
                    overview.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            return;
        }

        let visible = rect.intersect(canvas);
        if !visible.is_positive() {
            return;
        }
        let from = ((visible.min - rect.min) / scale).max(Vec2::ZERO);
        let to = (visible.max - rect.min) / scale;
        let max_texture = ui.ctx().input(|i| i.max_texture_side) as u32;
        let tile = TILE.min(max_texture.max(1));
        let x0 = from.x as u32 / tile;
        let y0 = from.y as u32 / tile;
        let x1 = (to.x.ceil() as u32).min(image.width()).div_ceil(tile);
        let y1 = (to.y.ceil() as u32).min(image.height()).div_ceil(tile);
        if self.tiles.len() >= 64 {
            self.tiles
                .retain(|&(x, y), _| (x0..x1).contains(&x) && (y0..y1).contains(&y));
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let left = x * tile;
                let top = y * tile;
                let w = tile.min(image.width() - left);
                let h = tile.min(image.height() - top);
                let texture = self.tiles.entry((x, y)).or_insert_with(|| {
                    let part = image::imageops::crop_imm(image, left, top, w, h).to_image();
                    ui.ctx().load_texture(
                        "Участок изображения",
                        color_image(&part),
                        TextureOptions {
                            magnification: egui::TextureFilter::Nearest,
                            ..TextureOptions::LINEAR
                        },
                    )
                });
                let tile_rect = Rect::from_min_size(
                    rect.min + vec2(left as f32, top as f32) * scale,
                    vec2(w as f32, h as f32) * scale,
                );
                painter.image(
                    texture.id(),
                    tile_rect,
                    Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    }
}

fn color_image(image: &RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    )
}

struct MetaPanel {
    original: Vec<Tag>,
    tags: Vec<Tag>,
    pending: Option<Receiver<Result<Vec<Tag>>>>,
    filter: String,
    new_key: String,
    new_value: String,
    error: Option<String>,
}

impl MetaPanel {
    fn new(ctx: &Context, path: PathBuf) -> Self {
        Self {
            original: Vec::new(),
            tags: Vec::new(),
            pending: Some(job(ctx, move || metadata::read(&path))),
            filter: String::new(),
            new_key: "EXIF:Artist".into(),
            new_value: String::new(),
            error: None,
        }
    }
}

pub struct Viewer {
    settings: Settings,
    palette: Palette,
    theme_light: bool,
    loaded: Option<Loaded>,
    load: Option<Receiver<Result<Loaded>>>,
    saving: Option<Receiver<Result<PathBuf>>>,
    textures: Textures,
    view: View,
    canvas: Rect,
    frame: usize,
    plays: u32,
    animation_paused: bool,
    deadline: Option<Instant>,
    dirty: bool,
    dirty_action: Option<DirtyAction>,
    menu: bool,
    integrations: bool,
    tools: bool,
    crop: bool,
    selection: Option<(Pos2, Pos2)>,
    meta: Option<MetaPanel>,
    status: String,
    error: Option<String>,
}

#[derive(Clone)]
enum DirtyAction {
    Open(PathBuf, u32),
    Close,
}

impl Viewer {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        file: Option<PathBuf>,
        integration_error: Option<String>,
    ) -> Self {
        set_fonts(&cc.egui_ctx);
        let light = match settings.theme {
            Theme::Auto => shell::system_light(),
            Theme::Light => true,
            Theme::Dark => false,
        };
        let palette = Palette::new(light);
        set_style(&cc.egui_ctx, light, palette);
        let mut this = Self {
            settings,
            palette,
            theme_light: light,
            loaded: None,
            load: None,
            saving: None,
            textures: Textures::default(),
            view: View::default(),
            canvas: Rect::NOTHING,
            frame: 0,
            plays: 0,
            animation_paused: false,
            deadline: None,
            dirty: false,
            dirty_action: None,
            menu: false,
            integrations: false,
            tools: false,
            crop: false,
            selection: None,
            meta: None,
            status: String::new(),
            error: integration_error,
        };
        if let Some(path) = file {
            this.start_open(&cc.egui_ctx, path, 0);
        }
        this
    }

    fn size(&self) -> Option<Vec2> {
        let image = &self.loaded.as_ref()?.picture.frames[self.frame].image;
        Some(vec2(image.width() as f32, image.height() as f32))
    }

    fn path(&self) -> Option<&Path> {
        Some(&self.loaded.as_ref()?.path)
    }

    fn busy(&self) -> bool {
        self.load.is_some() || self.saving.is_some()
    }

    fn request_open(&mut self, ctx: &Context, path: PathBuf, page: u32) {
        if self.busy() {
            return;
        }
        if self.dirty {
            self.dirty_action = Some(DirtyAction::Open(path, page));
        } else {
            self.start_open(ctx, path, page);
        }
    }

    fn start_open(&mut self, ctx: &Context, path: PathBuf, page: u32) {
        self.menu = false;
        self.tools = false;
        self.crop = false;
        self.selection = None;
        self.meta = None;
        self.load = Some(job(ctx, move || {
            let path = std::path::absolute(path)?;
            let picture = decode::decode(&path, page)?;
            let preview = preview(&picture.frames[0].image);
            let folder = sibling_files(&path);
            Ok(Loaded {
                path,
                picture,
                page,
                folder,
                preview,
            })
        }));
    }

    fn open_dialog(&mut self, ctx: &Context) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Открыть изображение")
            .add_filter("Изображения", decode::EXTENSIONS)
            .add_filter("Все файлы", &["*"]);
        if let Some(dir) = self.path().and_then(Path::parent) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.request_open(ctx, path, 0);
        }
    }

    fn save_as(&mut self, ctx: &Context) {
        if self.busy() {
            return;
        }
        let Some(loaded) = &self.loaded else { return };
        let name = loaded
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let mut dialog = rfd::FileDialog::new()
            .set_title("Сохранить изображение")
            .set_file_name(format!("{name}-edit.png"))
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("WebP", &["webp"])
            .add_filter("BMP", &["bmp"])
            .add_filter("TIFF", &["tiff", "tif"])
            .add_filter("QOI", &["qoi"]);
        if let Some(folder) = loaded.path.parent() {
            dialog = dialog.set_directory(folder);
        }
        if let Some(mut path) = dialog.save_file() {
            if path.extension().is_none() {
                path.set_extension("png");
            }
            let image = loaded.picture.frames[self.frame].image.clone();
            self.saving = Some(job(ctx, move || {
                decode::export(&image, &path)?;
                std::path::absolute(path).map_err(Into::into)
            }));
        }
    }

    fn refresh_texture(&mut self, ctx: &Context) {
        if let Some(loaded) = &self.loaded {
            self.textures.set(
                ctx,
                &loaded.picture.frames[self.frame].image,
                loaded.preview.as_ref(),
            );
        }
    }

    fn folder_index(&self) -> Option<(usize, usize)> {
        let loaded = self.loaded.as_ref()?;
        let index = loaded.folder.iter().position(|file| file == &loaded.path)?;
        Some((index, loaded.folder.len()))
    }

    fn step_file(&mut self, ctx: &Context, delta: isize) {
        let Some((index, len)) = self.folder_index() else {
            return;
        };
        if len > 1 {
            let index = (index as isize + delta).rem_euclid(len as isize) as usize;
            let path = self.loaded.as_ref().unwrap().folder[index].clone();
            self.request_open(ctx, path, 0);
        }
    }

    fn step_page(&mut self, ctx: &Context, delta: i64) {
        if let Some(loaded) = &self.loaded {
            if loaded.picture.pages > 1 {
                let target =
                    (i64::from(loaded.page) + delta).rem_euclid(i64::from(loaded.picture.pages));
                self.request_open(ctx, loaded.path.clone(), target as u32);
            }
        }
    }

    fn change(&mut self, ctx: &Context, operation: impl FnOnce(&RgbaImage) -> RgbaImage) {
        if self.busy() {
            return;
        }
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        let image = operation(&loaded.picture.frames[self.frame].image);
        loaded.preview = preview(&image);
        loaded.picture.frames.truncate(1);
        loaded.picture.frames[0].image = image;
        self.frame = 0;
        self.deadline = None;
        self.dirty = true;
        self.status.clear();
        self.crop = false;
        self.selection = None;
        self.refresh_texture(ctx);
        if let Some(size) = self.size() {
            self.view.fit(self.canvas, size);
        }
        self.update_title(ctx);
    }

    fn toggle_crop(&mut self) {
        if self.loaded.is_some() {
            self.crop = !self.crop;
            self.selection = None;
        }
    }

    fn apply_crop(&mut self, ctx: &Context) {
        let Some((start, end)) = self.selection else {
            return;
        };
        let Some(size) = self.size() else { return };
        let rect = Rect::from_two_pos(start, end);
        let start = rect.min.to_vec2().floor().max(Vec2::ZERO);
        let end = rect.max.to_vec2().ceil().min(size);
        let width = (end.x - start.x) as u32;
        let height = (end.y - start.y) as u32;
        if width > 0 && height > 0 {
            self.change(ctx, |image| {
                image::imageops::crop_imm(image, start.x as u32, start.y as u32, width, height)
                    .to_image()
            });
        }
    }

    fn update_title(&self, ctx: &Context) {
        let title = self
            .path()
            .and_then(Path::file_name)
            .map(|name| {
                format!(
                    "{}{} — ThodView",
                    if self.dirty { "* " } else { "" },
                    name.to_string_lossy()
                )
            })
            .unwrap_or_else(|| "ThodView".into());
        ctx.send_viewport_cmd(ViewportCommand::Title(title));
    }

    fn receive(&mut self, ctx: &Context) {
        if let Some(Ok(result)) = self.load.as_ref().map(Receiver::try_recv) {
            self.load = None;
            match result {
                Ok(loaded) => {
                    self.frame = 0;
                    self.plays = 0;
                    self.animation_paused = false;
                    self.deadline = (loaded.picture.frames.len() > 1)
                        .then(|| Instant::now() + loaded.picture.frames[0].delay);
                    self.status = loaded.picture.note.unwrap_or("").into();
                    self.loaded = Some(loaded);
                    self.dirty = false;
                    self.refresh_texture(ctx);
                    self.update_title(ctx);
                    self.view = View::default();
                    if self.canvas.is_positive() {
                        self.view.fit(self.canvas, self.size().unwrap());
                    }
                }
                Err(err) => self.error = Some(err.to_string()),
            }
        }
        if let Some(Ok(result)) = self.saving.as_ref().map(Receiver::try_recv) {
            self.saving = None;
            match result {
                Ok(path) => {
                    self.status = format!(
                        "Сохранено: {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    self.dirty = false;
                    if let Some(loaded) = &mut self.loaded {
                        loaded.folder = sibling_files(&path);
                        loaded.path = path;
                        loaded.page = 0;
                        loaded.picture.pages = 1;
                    }
                    self.update_title(ctx);
                }
                Err(err) => {
                    self.dirty_action = None;
                    self.error = Some(err.to_string());
                }
            }
            if !self.dirty {
                if let Some(action) = self.dirty_action.take() {
                    self.finish_action(ctx, action);
                }
            }
        }
    }

    fn tick_animation(&mut self, ctx: &Context) {
        let Some(deadline) = self.deadline else {
            return;
        };
        if self.animation_paused || self.crop || self.busy() || self.meta.is_some() {
            return;
        }
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        let now = Instant::now();
        if deadline <= now {
            let next = (self.frame + 1) % loaded.picture.frames.len();
            if next == 0 {
                self.plays += 1;
                if loaded
                    .picture
                    .plays
                    .is_some_and(|limit| self.plays >= limit)
                {
                    self.deadline = None;
                    return;
                }
            }
            self.frame = next;
            loaded.preview = preview(&loaded.picture.frames[next].image);
            self.deadline = Some(now + loaded.picture.frames[next].delay);
            self.refresh_texture(ctx);
        }
        if let Some(deadline) = self.deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
        }
    }

    fn show_meta(&mut self, ctx: &Context) {
        if let Some(path) = self.path() {
            self.meta = Some(MetaPanel::new(ctx, path.to_owned()));
        }
    }

    fn reveal(&mut self) {
        if let Some(path) = self.path() {
            if let Err(err) = shell::reveal(path) {
                self.error = Some(err.to_string());
            }
        }
    }

    fn toggle_theme(&mut self, ctx: &Context) {
        self.settings.theme = self.settings.theme.next();
        self.update_theme(ctx);
        self.save_settings();
    }

    fn update_theme(&mut self, ctx: &Context) {
        let light = match self.settings.theme {
            Theme::Auto => ctx
                .input(|i| i.raw.system_theme)
                .map_or_else(shell::system_light, |theme| theme == egui::Theme::Light),
            Theme::Light => true,
            Theme::Dark => false,
        };
        if self.theme_light != light {
            self.theme_light = light;
            self.palette = Palette::new(light);
            set_style(ctx, light, self.palette);
        }
    }

    fn save_settings(&mut self) {
        if let Err(err) = self.settings.save() {
            self.error = Some(err.to_string());
        }
    }

    fn integrations(&mut self, ctx: &Context) {
        let mut open = self.integrations;
        let mut desired = self.settings.context_menu;
        let mut defaults = false;
        let mut apply = false;
        egui::Window::new("Интеграция с Windows")
            .open(&mut open)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                apply |= ui
                    .checkbox(&mut desired, "Открыть в ThodView в меню Проводника")
                    .changed();
                ui.label(
                    "В Windows 11 пункт находится в меню «Показать дополнительные параметры».",
                );
                if ui.button("Зарегистрировать ThodView").clicked() {
                    apply = true;
                }
                if ui.button("Выбрать ThodView по умолчанию…").clicked() {
                    defaults = true;
                }
            });
        self.integrations = open;
        if apply || defaults {
            match shell::register(desired, true)
                .and_then(|_| if defaults { shell::defaults() } else { Ok(()) })
            {
                Ok(()) => {
                    self.settings.context_menu = desired;
                    self.save_settings();
                    self.status = "ThodView зарегистрирован в Windows".to_owned();
                }
                Err(err) => self.error = Some(err.to_string()),
            }
        }
    }

    fn finish_action(&mut self, ctx: &Context, action: DirtyAction) {
        match action {
            DirtyAction::Open(path, page) => self.start_open(ctx, path, page),
            DirtyAction::Close => {
                self.dirty = false;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn dirty_dialog(&mut self, ctx: &Context) {
        let Some(action) = self.dirty_action.clone() else {
            return;
        };
        if self.saving.is_some() {
            return;
        }
        egui::Modal::new(Id::new("unsaved")).show(ctx, |ui| {
            ui.heading("Есть несохранённые изменения");
            ui.horizontal(|ui| {
                if ui.button("Сохранить…").clicked() {
                    self.save_as(ctx);
                }
                if ui.button("Продолжить без сохранения").clicked() {
                    self.dirty_action = None;
                    self.finish_action(ctx, action);
                }
                if ui.button("Отмена").clicked() {
                    self.dirty_action = None;
                }
            });
        });
    }
}

impl Viewer {
    fn title(&mut self, ui: &mut Ui, rect: Rect) {
        let p = self.palette;
        let ctx = ui.ctx().clone();
        let painter = ui.painter();
        painter.rect_filled(rect, 0, p.panel);
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.line),
        );
        painter.text(
            rect.left_center() + vec2(14.0, 0.0),
            Align2::LEFT_CENTER,
            "ThodView",
            FontId::proportional(12.0),
            p.muted,
        );
        painter.vline(
            rect.left() + 75.0,
            (rect.center().y - 6.5)..=(rect.center().y + 6.5),
            Stroke::new(1.0_f32, p.line),
        );
        let caption = Rect::from_min_max(
            rect.min + vec2(86.0, 0.0),
            rect.right_bottom() - vec2(148.0, 0.0),
        );
        let name = self
            .path()
            .and_then(Path::file_name)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Ничего не открыто".into());
        let name = if self.dirty {
            format!("{name} •")
        } else {
            name
        };
        painter.with_clip_rect(caption).text(
            caption.left_center(),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(12.5),
            p.text,
        );
        let drag = Rect::from_min_max(
            rect.min + vec2(6.0, 6.0),
            rect.right_bottom() - vec2(148.0, 0.0),
        );
        let response = ui.interact(drag, Id::new("title-drag"), Sense::click_and_drag());
        if response.double_clicked() {
            toggle_maximize(&ctx);
        } else if response.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        for (index, glyph, tip) in [
            (0, '\u{e921}', "Свернуть"),
            (1, if max { '\u{e923}' } else { '\u{e922}' }, "Развернуть"),
            (2, '\u{e8bb}', "Закрыть"),
        ] {
            let r = Rect::from_min_size(
                pos2(rect.right() - 144.0 + index as f32 * 46.0, rect.top() + 6.0),
                vec2(46.0, 32.0),
            );
            if ui
                .put(r, Button::new(icon(glyph, 10.0)).frame(false))
                .on_hover_text(tip)
                .clicked()
            {
                match index {
                    0 => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                    1 => toggle_maximize(&ctx),
                    _ if self.dirty || self.saving.is_some() => {
                        self.dirty_action = Some(DirtyAction::Close)
                    }
                    _ => ctx.send_viewport_cmd(ViewportCommand::Close),
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let p = self.palette;
        let painter = ui.painter();
        painter.rect_filled(rect, 0, p.panel);
        painter.hline(
            rect.x_range(),
            rect.top() + 0.5,
            Stroke::new(1.0_f32, p.line),
        );
        let left = if !self.status.is_empty() {
            self.status.clone()
        } else {
            self.path()
                .and_then(Path::file_name)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Готово".into())
        };
        let label = Rect::from_min_size(
            rect.min + vec2(14.0, 0.0),
            vec2((rect.width() / 2.0 - 130.0).min(440.0), STATUS),
        );
        painter.with_clip_rect(label).text(
            label.left_center(),
            Align2::LEFT_CENTER,
            left,
            FontId::proportional(11.5),
            p.muted,
        );
        let Some(size) = self.size() else {
            painter.text(
                rect.right_center() - vec2(14.0, 0.0),
                Align2::RIGHT_CENTER,
                "—",
                FontId::proportional(11.5),
                p.muted,
            );
            return;
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            format!("{} × {}", size.x, size.y),
            FontId::proportional(11.5),
            p.muted,
        );
        let zoom = (self.view.scale * ui.ctx().pixels_per_point() * 100.0).round();
        painter.text(
            rect.right_center() - vec2(14.0, 0.0),
            Align2::RIGHT_CENTER,
            format!("{zoom}%"),
            FontId::proportional(11.5),
            p.muted,
        );
        let button =
            Rect::from_center_size(rect.right_center() - vec2(90.0, 0.0), vec2(24.0, 24.0));
        if ui
            .put(button, Button::new(icon('\u{e8b7}', 13.0)).frame(false))
            .on_hover_text("Открыть папку с файлом (F)")
            .clicked()
        {
            self.reveal();
        }
    }

    fn chrome(&mut self, ctx: &Context, canvas: Rect, zoomed: bool) -> Vec<Rect> {
        let p = self.palette;
        let mut blocked = Vec::new();
        if !zoomed {
            blocked.push(floating(
                ctx,
                "gear",
                pos2(canvas.right() - 54.0, canvas.top() + 14.0),
                |ui| {
                    if ui
                        .add_sized([40.0, 40.0], panel_button(icon('\u{e713}', 16.0), p))
                        .on_hover_text("Настройки и действия")
                        .clicked()
                    {
                        self.menu = !self.menu;
                    }
                },
            ));
            if self.menu {
                blocked.push(floating(
                    ctx,
                    "gear-panel",
                    pos2(canvas.right() - 266.0, canvas.top() + 62.0),
                    |ui| {
                        panel_frame(p, 12, 7).show(ui, |ui| {
                            ui.set_width(236.0);
                            let loaded = self.loaded.is_some() && !self.busy();
                            if menu_row(ui, "Открыть…", "Ctrl+O", true, p) {
                                self.menu = false;
                                self.open_dialog(ctx);
                            }
                            if menu_row(ui, "Сохранить как…", "Ctrl+S", loaded, p) {
                                self.menu = false;
                                self.save_as(ctx);
                            }
                            if menu_row(ui, "Открыть папку с файлом", "F", loaded, p)
                            {
                                self.menu = false;
                                self.reveal();
                            }
                            ui.separator();
                            if menu_row(ui, "Вписать в окно", "0", loaded, p) {
                                if let Some(size) = self.size() {
                                    self.view.fit(canvas, size);
                                }
                                self.menu = false;
                            }
                            if menu_row(ui, "Реальный размер", "1", loaded, p) {
                                if let Some(size) = self.size() {
                                    self.view.actual(canvas, size, ctx.pixels_per_point());
                                }
                                self.menu = false;
                            }
                            ui.separator();
                            if menu_row(ui, "Метаданные…", "M", loaded, p) {
                                self.menu = false;
                                self.show_meta(ctx);
                            }
                            if menu_row(ui, "Открывать изображения…", "Windows", true, p)
                            {
                                self.menu = false;
                                self.integrations = true;
                            }
                            ui.separator();
                            if menu_row(
                                ui,
                                &format!("Тема: {}", self.settings.theme.label()),
                                "T",
                                true,
                                p,
                            ) {
                                self.toggle_theme(ctx);
                            }
                        });
                    },
                ));
            }

            if self.loaded.is_some() && !self.crop {
                blocked.push(floating(
                    ctx,
                    "edit-toggle",
                    pos2(canvas.left() + 14.0, canvas.bottom() - 54.0),
                    |ui| {
                        if ui
                            .add_sized([40.0, 40.0], panel_button(icon('\u{e70f}', 16.0), p))
                            .on_hover_text("Показать инструменты (поворот, отражение, обрезка)")
                            .clicked()
                        {
                            self.tools = !self.tools;
                        }
                    },
                ));
                if self.tools {
                    blocked.push(floating(
                        ctx,
                        "edit-tools",
                        pos2(canvas.left() + 64.0, canvas.bottom() - 60.0),
                        |ui| {
                            panel_frame(p, 13, 4).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if tool_button(ui, "↺", "Повернуть влево (Shift+R)", p)
                                    {
                                        self.change(ctx, image::imageops::rotate270);
                                    }
                                    if tool_button(ui, "↻", "Повернуть вправо (R)", p)
                                    {
                                        self.change(ctx, image::imageops::rotate90);
                                    }
                                    ui.separator();
                                    if tool_button(ui, "⇋", "Отразить по горизонтали (H)", p)
                                    {
                                        self.change(ctx, image::imageops::flip_horizontal);
                                    }
                                    if tool_button(ui, "⇅", "Отразить по вертикали (V)", p)
                                    {
                                        self.change(ctx, image::imageops::flip_vertical);
                                    }
                                    ui.separator();
                                    if tool_button(ui, "⛶", "Обрезать (C)", p) {
                                        self.toggle_crop();
                                    }
                                    if ui
                                        .add_sized(
                                            [38.0, 38.0],
                                            panel_button(icon('\u{e946}', 16.0), p),
                                        )
                                        .on_hover_text("Метаданные (M)")
                                        .clicked()
                                    {
                                        self.show_meta(ctx);
                                    }
                                });
                            });
                        },
                    ));
                }
            }
        }
        if self.crop {
            blocked.push(floating(
                ctx,
                "crop-apply",
                pos2(canvas.center().x - 175.0, canvas.bottom() - 58.0),
                |ui| {
                    panel_frame(p, 11, 9).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Выделите область мышью")
                                    .size(12.0)
                                    .color(p.muted),
                            );
                            if ui
                                .add_enabled(
                                    self.selection.is_some(),
                                    Button::new(RichText::new("Обрезать").color(Color32::WHITE))
                                        .fill(p.accent),
                                )
                                .clicked()
                            {
                                self.apply_crop(ctx);
                            }
                            if ui.button("Отмена").clicked() {
                                self.crop = false;
                                self.selection = None;
                            }
                        });
                    });
                },
            ));
        } else {
            let nav_hover = ctx
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|at| canvas.contains(at) && at.y > canvas.bottom() - 80.0);
            if nav_hover && !self.busy() {
                let pages = self.loaded.as_ref().map_or(1, |l| l.picture.pages);
                let current = self.loaded.as_ref().map_or(0, |l| l.page) + 1;
                let folder = self.folder_index().filter(|&(_, count)| count > 1);
                let is_movie = self
                    .loaded
                    .as_ref()
                    .is_some_and(|l| l.picture.frames.len() > 1);
                if folder.is_some() || pages > 1 || is_movie {
                    let width = if folder.is_none() { 0.0 } else { 150.0 }
                        + if pages > 1 { 148.0 } else { 0.0 }
                        + if is_movie { 48.0 } else { 0.0 };
                    blocked.push(floating(
                        ctx,
                        "folder-nav",
                        pos2(canvas.center().x - width * 0.5, canvas.bottom() - 61.0),
                        |ui| {
                            panel_frame(p, 14, 5).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if let Some((index, count)) = folder {
                                        if nav_button(ui, '\u{e76b}', "Предыдущее изображение (←)")
                                        {
                                            self.step_file(ctx, -1);
                                        }
                                        nav_label(ui, format!("{} / {}", index + 1, count), p);
                                        if nav_button(ui, '\u{e76c}', "Следующее изображение (→)")
                                        {
                                            self.step_file(ctx, 1);
                                        }
                                    }
                                    if pages > 1 {
                                        if folder.is_some() {
                                            ui.separator();
                                        }
                                        if nav_button(
                                            ui,
                                            '\u{e76b}',
                                            "Предыдущая страница (Page Up)",
                                        ) {
                                            self.step_page(ctx, -1);
                                        }
                                        nav_label(ui, format!("{current} / {pages}"), p);
                                        if nav_button(
                                            ui,
                                            '\u{e76c}',
                                            "Следующая страница (Page Down)",
                                        ) {
                                            self.step_page(ctx, 1);
                                        }
                                    }
                                    if is_movie {
                                        if folder.is_some() {
                                            ui.separator();
                                        }
                                        if nav_button(
                                            ui,
                                            if self.animation_paused {
                                                '\u{e768}'
                                            } else {
                                                '\u{e769}'
                                            },
                                            "Анимация (Пробел)",
                                        ) {
                                            self.toggle_animation();
                                        }
                                    }
                                });
                            });
                        },
                    ));
                }
            }
        }
        blocked
    }

    fn show_pixels(&mut self, ui: &mut Ui, canvas: Rect, blocked: &[Rect], allow_input: bool) {
        let ctx = ui.ctx().clone();
        let p = self.palette;
        ui.painter().rect_filled(canvas, 0, p.canvas);
        let Some(size) = self.size() else {
            if self.load.is_none() {
                let center = canvas.center();
                ui.painter().text(
                    center - vec2(0.0, 44.0),
                    Align2::CENTER_CENTER,
                    "Откройте изображение",
                    FontId::proportional(24.0),
                    p.text,
                );
                ui.painter().text(
                    center - vec2(0.0, 11.0),
                    Align2::CENTER_CENTER,
                    "Перетащите изображение сюда",
                    FontId::proportional(13.0),
                    p.muted,
                );
                let button = Rect::from_center_size(center + vec2(0.0, 33.0), vec2(136.0, 34.0));
                if ui
                    .put(
                        button,
                        Button::new(RichText::new("Выбрать файл…").color(Color32::WHITE))
                            .fill(p.accent)
                            .corner_radius(8),
                    )
                    .clicked()
                    && allow_input
                {
                    self.open_dialog(&ctx);
                }
            }
            return;
        };

        if allow_input {
            let response = ui.interact(canvas, Id::new("image-viewport"), Sense::click_and_drag());
            let pointer = ctx.input(|i| i.pointer.hover_pos());
            let available = pointer.is_some_and(|point| {
                canvas.contains(point) && !blocked.iter().any(|area| area.contains(point))
            });
            let pressed = ctx.input(|i| i.pointer.primary_pressed());
            if self.menu && available && pressed {
                self.menu = false;
            }
            if !self.crop && available {
                let steps = ctx.input(|i| {
                    i.events
                        .iter()
                        .filter_map(|event| match event {
                            egui::Event::MouseWheel { unit, delta, .. } => Some(
                                delta.y
                                    * match unit {
                                        egui::MouseWheelUnit::Line => 1.0,
                                        egui::MouseWheelUnit::Point => 1.0 / 55.0,
                                        egui::MouseWheelUnit::Page => 3.0,
                                    },
                            ),
                            _ => None,
                        })
                        .sum::<f32>()
                });
                if steps != 0.0 {
                    self.view.zoom(
                        canvas,
                        size,
                        pointer.unwrap(),
                        1.2_f32.powf(steps.clamp(-40.0, 40.0)),
                    );
                    if self.view.is_zoomed(canvas, size) {
                        self.menu = false;
                        self.tools = false;
                    }
                }
                if response.double_clicked() {
                    if self.view.is_zoomed(canvas, size) {
                        self.view.fit(canvas, size);
                    } else {
                        self.view.actual(canvas, size, ctx.pixels_per_point());
                    }
                }
            }
            if self.crop {
                let pixel = |point: Pos2| {
                    let pixel =
                        ((point - self.view.origin) / self.view.scale).clamp(Vec2::ZERO, size);
                    pos2(pixel.x, pixel.y)
                };
                if available && pressed {
                    let at = pixel(pointer.unwrap());
                    self.selection = Some((at, at));
                }
                if response.dragged() || response.drag_stopped() {
                    if let (Some(point), Some((_, end))) = (pointer, &mut self.selection) {
                        *end = pixel(point);
                    }
                }
            } else if response.dragged() {
                self.view.pan(canvas, size, response.drag_delta());
            }
        }

        let image_rect = Rect::from_min_size(self.view.origin, size * self.view.scale);
        let loaded = self.loaded.as_ref().unwrap();
        self.textures.paint(
            ui,
            image_rect,
            canvas,
            &loaded.picture.frames[self.frame].image,
            self.view.scale,
        );
        if self.crop {
            if let Some((start, end)) = self.selection {
                let painter = ui.painter().with_clip_rect(canvas);
                let selection = Rect::from_two_pos(
                    self.view.origin + start.to_vec2() * self.view.scale,
                    self.view.origin + end.to_vec2() * self.view.scale,
                );
                let dim = Color32::from_black_alpha(122);
                for rect in [
                    Rect::from_min_max(image_rect.min, pos2(image_rect.right(), selection.top())),
                    Rect::from_min_max(pos2(image_rect.left(), selection.bottom()), image_rect.max),
                    Rect::from_min_max(
                        pos2(image_rect.left(), selection.top()),
                        selection.left_bottom(),
                    ),
                    Rect::from_min_max(
                        selection.right_top(),
                        pos2(image_rect.right(), selection.bottom()),
                    ),
                ] {
                    painter.rect_filled(rect, 0, dim);
                }
                painter.rect_stroke(
                    selection,
                    0,
                    Stroke::new(1.5_f32, p.accent),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    fn loading_overlay(&self, ui: &Ui) {
        if !self.busy() {
            return;
        }
        let p = self.palette;
        let center = self.canvas.center();
        ui.painter()
            .rect_filled(self.canvas, 0, p.canvas.gamma_multiply(0.94));
        let bar = Rect::from_center_size(center - vec2(0.0, 9.0), vec2(200.0, 4.0));
        ui.painter().rect_filled(bar, 2, p.line);
        let t = ui.ctx().input(|i| i.time as f32);
        let offset = (1.0 - (t * 4.0).cos()) * 67.0;
        ui.painter().rect_filled(
            Rect::from_min_size(bar.min + vec2(offset, 0.0), vec2(66.0, 4.0)),
            2,
            p.accent,
        );
        ui.painter().text(
            center + vec2(0.0, 17.0),
            Align2::CENTER_CENTER,
            if self.load.is_some() {
                "Открываю изображение…"
            } else {
                "Сохраняю изображение…"
            },
            FontId::proportional(12.5),
            p.muted,
        );
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }

    fn toggle_animation(&mut self) {
        let Some(loaded) = &self.loaded else { return };
        if loaded.picture.frames.len() > 1 {
            if self.deadline.is_none() {
                self.frame = loaded.picture.frames.len() - 1;
                self.plays = 0;
                self.deadline = Some(Instant::now());
                self.animation_paused = false;
            } else {
                self.animation_paused = !self.animation_paused;
            }
        }
    }

    fn shortcuts(&mut self, ctx: &Context) {
        if self.error.is_some()
            || self.meta.is_some()
            || self.integrations
            || self.dirty_action.is_some()
            || self.busy()
        {
            return;
        }
        for event in ctx.input(|i| i.events.clone()) {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                repeat,
                ..
            } = event
            else {
                continue;
            };
            if modifiers.command {
                match key {
                    Key::O if !repeat => self.open_dialog(ctx),
                    Key::S if !repeat => self.save_as(ctx),
                    _ => {}
                }
                continue;
            }
            match key {
                Key::ArrowLeft if !self.crop => self.step_file(ctx, -1),
                Key::ArrowRight if !self.crop => self.step_file(ctx, 1),
                Key::PageUp if !self.crop => self.step_page(ctx, -1),
                Key::PageDown if !self.crop => self.step_page(ctx, 1),
                Key::F if !repeat => self.reveal(),
                Key::M if !repeat => self.show_meta(ctx),
                Key::R if !repeat && modifiers.shift => {
                    self.change(ctx, image::imageops::rotate270)
                }
                Key::R if !repeat => self.change(ctx, image::imageops::rotate90),
                Key::H if !repeat => self.change(ctx, image::imageops::flip_horizontal),
                Key::V if !repeat => self.change(ctx, image::imageops::flip_vertical),
                Key::C if !repeat => self.toggle_crop(),
                Key::Enter if self.crop => self.apply_crop(ctx),
                Key::Space if !repeat => self.toggle_animation(),
                Key::T if !repeat => self.toggle_theme(ctx),
                Key::Escape => {
                    self.crop = false;
                    self.selection = None;
                    self.menu = false;
                }
                Key::Num0 | Key::Num1 | Key::Plus | Key::Equals | Key::Minus
                    if self.canvas.is_positive() =>
                {
                    if let Some(size) = self.size() {
                        match key {
                            Key::Num0 => self.view.fit(self.canvas, size),
                            Key::Num1 => {
                                self.view.actual(self.canvas, size, ctx.pixels_per_point())
                            }
                            _ => self.view.zoom(
                                self.canvas,
                                size,
                                self.canvas.center(),
                                if key == Key::Minus { 0.8 } else { 1.25 },
                            ),
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn metadata_window(&mut self, ctx: &Context) {
        let Some(path) = self.path().map(Path::to_owned) else {
            return;
        };
        let Some(meta) = &mut self.meta else { return };
        if let Some(Ok(result)) = meta.pending.as_ref().map(Receiver::try_recv) {
            meta.pending = None;
            match result {
                Ok(tags) => {
                    meta.original = tags.clone();
                    meta.tags = tags;
                }
                Err(err) => meta.error = Some(err.to_string()),
            }
        }
        let mut open = true;
        let palette = self.palette;
        egui::Window::new("Метаданные изображения")
            .open(&mut open)
            .default_width(780.0)
            .default_height(520.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                if let Some(error) = &meta.error {
                    ui.colored_label(palette.accent, error);
                }
                if meta.pending.is_some() {
                    ui.spinner();
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Поиск");
                    ui.text_edit_singleline(&mut meta.filter);
                    if ui.button("Скопировать").clicked() {
                        ctx.copy_text(
                            meta.tags
                                .iter()
                                .map(|t| format!("{} = {}", t.key, t.value))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                    if ui.button("Записать изменения").clicked() {
                        let original = meta.original.clone();
                        let tags = meta.tags.clone();
                        let path = path.clone();
                        meta.error = None;
                        meta.pending = Some(job(ctx, move || {
                            metadata::write(&path, &original, &tags)?;
                            metadata::read(&path)
                        }));
                    }
                    if ui.button("Удалить всё…").clicked()
                        && rfd::MessageDialog::new()
                            .set_title("Удалить метаданные")
                            .set_description("Удалить все доступные метаданные из исходного файла?")
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            == rfd::MessageDialogResult::Yes
                    {
                        let path = path.clone();
                        meta.error = None;
                        meta.pending = Some(job(ctx, move || {
                            metadata::strip(&path)?;
                            metadata::read(&path)
                        }));
                    }
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut meta.new_key)
                            .desired_width(130.0)
                            .hint_text("Группа:Тег"),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut meta.new_value)
                            .desired_width(220.0)
                            .hint_text("Значение"),
                    );
                    if ui.button("Добавить поле").clicked() {
                        if !metadata::valid_key(&meta.new_key)
                            || meta
                                .tags
                                .iter()
                                .any(|tag| tag.key.eq_ignore_ascii_case(&meta.new_key))
                        {
                            meta.error =
                                Some("Укажите уникальное имя поля в виде Группа:Тег".into());
                        } else {
                            meta.error = None;
                            meta.tags.push(Tag {
                                key: meta.new_key.clone(),
                                label: meta.new_key.clone(),
                                value: std::mem::take(&mut meta.new_value),
                                editable: true,
                            });
                        }
                    }
                });
                let filter = meta.filter.to_lowercase();
                let mut remove = None;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, tag) in meta.tags.iter_mut().enumerate() {
                            if !filter.is_empty()
                                && !tag.key.to_lowercase().contains(&filter)
                                && !tag.label.to_lowercase().contains(&filter)
                                && !tag.value.to_lowercase().contains(&filter)
                            {
                                continue;
                            }
                            ui.push_id(index, |ui| {
                                ui.horizontal(|ui| {
                                    let name = egui::Label::new(&tag.label).truncate();
                                    ui.add_sized([180.0, 23.0], name).on_hover_text(&tag.key);
                                    if tag.editable {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut tag.value)
                                                .desired_width(
                                                    (ui.available_width() - 30.0).max(80.0),
                                                ),
                                        );
                                        if ui.button("×").on_hover_text("Удалить поле").clicked()
                                        {
                                            remove = Some(index);
                                        }
                                    } else {
                                        ui.add(egui::Label::new(&tag.value).truncate())
                                            .on_hover_text(&tag.value);
                                    }
                                })
                            });
                        }
                    });
                if let Some(index) = remove {
                    meta.tags.remove(index);
                }
            });
        if (!open || ctx.input(|i| i.key_pressed(Key::Escape))) && meta.pending.is_none() {
            self.meta = None;
        }
    }
}

impl eframe::App for Viewer {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        self.update_theme(ctx);
        let viewport = ctx.input(|i| i.viewport().clone());
        self.settings.maximized = viewport.maximized.unwrap_or(false);
        if !self.settings.maximized {
            if let Some(rect) = viewport.inner_rect.filter(|r| r.is_positive()) {
                self.settings.width = rect.width();
                self.settings.height = rect.height();
            }
        }
        if viewport.close_requested() && (self.dirty || self.saving.is_some()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.dirty_action = Some(DirtyAction::Close);
        }
        if self.meta.is_none() && self.dirty_action.is_none() {
            if let Some(path) =
                ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()))
            {
                self.request_open(ctx, path, 0);
            }
        }
        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(ctx, |ui| {
                let all = ui.max_rect();
                let title = Rect::from_min_max(all.min, pos2(all.right(), all.top() + TITLE));
                let status = Rect::from_min_max(pos2(all.left(), all.bottom() - STATUS), all.max);
                let canvas = Rect::from_min_max(
                    pos2(all.left(), title.bottom()),
                    pos2(all.right(), status.top()),
                );
                if !canvas.is_positive() || canvas.width() < 8.0 {
                    return;
                }
                if self.canvas != canvas {
                    if let Some(size) = self.size() {
                        if self.canvas.is_positive() {
                            self.view.resize(self.canvas, canvas, size);
                        } else {
                            self.view.fit(canvas, size);
                        }
                    }
                    self.canvas = canvas;
                }
                self.shortcuts(ctx);
                self.tick_animation(ctx);
                let zoomed = self
                    .size()
                    .is_some_and(|size| self.view.is_zoomed(canvas, size));
                let clear = self.error.is_none()
                    && self.meta.is_none()
                    && !self.integrations
                    && self.dirty_action.is_none()
                    && !self.busy();
                let blocked = if clear {
                    self.chrome(ctx, canvas, zoomed)
                } else {
                    Vec::new()
                };
                self.show_pixels(ui, canvas, &blocked, clear);
                self.loading_overlay(ui);
                self.title(ui, title);
                self.status_bar(ui, status);
                if !self.settings.maximized && clear {
                    borders(ui, all);
                }
            });

        if self.integrations {
            self.integrations(ctx);
        }
        self.metadata_window(ctx);
        self.dirty_dialog(ctx);
        if let Some(error) = self.error.clone() {
            let result = egui::Modal::new(Id::new("error")).show(ctx, |ui| {
                ui.heading("ThodView");
                ui.set_max_width(530.0);
                ui.label(error);
                if ui.button("Закрыть").clicked() {
                    self.error = None;
                }
            });
            if result.should_close() {
                self.error = None;
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.settings.save();
    }
}

fn set_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(root) = std::env::var_os("WINDIR") {
        let folder = Path::new(&root).join("Fonts");
        if let Ok(font) = std::fs::read(folder.join("segoeui.ttf")) {
            fonts
                .font_data
                .insert("segoe".into(), egui::FontData::from_owned(font).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoe".into());
        }
        if let Ok(font) = std::fs::read(folder.join("segmdl2.ttf")) {
            fonts
                .font_data
                .insert("mdl".into(), egui::FontData::from_owned(font).into());
            fonts.families.insert(
                egui::FontFamily::Name("windows-icons".into()),
                vec!["mdl".into()],
            );
        }
        if let Ok(font) = std::fs::read(folder.join("seguisym.ttf")) {
            fonts.font_data.insert(
                "segoe-symbol".into(),
                egui::FontData::from_owned(font).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("segoe-symbol".into());
        }
    }
    if !fonts
        .families
        .contains_key(&egui::FontFamily::Name("windows-icons".into()))
    {
        fonts.families.insert(
            egui::FontFamily::Name("windows-icons".into()),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
    }
    ctx.set_fonts(fonts);
}

fn set_style(ctx: &Context, light: bool, p: Palette) {
    let theme = if light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    ctx.set_theme(theme);
    ctx.style_mut_of(theme, |style| {
        style.visuals.override_text_color = Some(p.text);
        style.visuals.panel_fill = p.panel;
        style.visuals.window_fill = p.panel;
        style.visuals.window_stroke = Stroke::new(1.0_f32, p.line);
        style.visuals.widgets.inactive.bg_fill = p.panel;
        style.visuals.widgets.hovered.weak_bg_fill = p.hover;
        style.visuals.widgets.hovered.bg_fill = p.hover;
        style.visuals.widgets.active.bg_fill = p.hover;
        style.visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    });
}

fn toggle_maximize(ctx: &Context) {
    ctx.send_viewport_cmd(ViewportCommand::Maximized(
        !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
    ));
}

fn floating(ctx: &Context, name: &str, at: Pos2, render: impl FnOnce(&mut Ui)) -> Rect {
    Area::new(Id::new(name))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .show(ctx, render)
        .response
        .rect
}

fn panel_frame(p: Palette, radius: u8, margin: i8) -> Frame {
    Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(radius)
        .inner_margin(egui::Margin::same(margin))
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(56),
        })
}

fn panel_button(label: RichText, p: Palette) -> Button<'static> {
    Button::new(label)
        .fill(p.panel)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(8)
}

fn tool_button(ui: &mut Ui, label: &str, hint: &str, p: Palette) -> bool {
    ui.add_sized(
        [38.0, 38.0],
        panel_button(RichText::new(label).size(19.0), p),
    )
    .on_hover_text(hint)
    .clicked()
}

fn nav_button(ui: &mut Ui, glyph: char, hint: &str) -> bool {
    ui.add_sized([36.0, 34.0], Button::new(icon(glyph, 14.0)).frame(false))
        .on_hover_text(hint)
        .clicked()
}

fn nav_label(ui: &mut Ui, text: String, p: Palette) {
    ui.add_sized(
        [53.0, 34.0],
        egui::Label::new(RichText::new(text).size(11.5).color(p.muted)),
    );
}

fn menu_row(ui: &mut Ui, label: &str, binding: &str, enabled: bool, p: Palette) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        vec2(236.0, 30.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if response.hovered() && enabled {
        ui.painter().rect_filled(rect, 8, p.hover);
    }
    ui.painter().text(
        rect.left_center() + vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        if enabled { p.text } else { p.muted },
    );
    ui.painter().text(
        rect.right_center() - vec2(8.0, 0.0),
        Align2::RIGHT_CENTER,
        binding,
        FontId::proportional(11.0),
        p.muted,
    );
    response.clicked()
}

fn borders(ui: &mut Ui, rect: Rect) {
    use egui::{CursorIcon, ResizeDirection::*};
    let ctx = ui.ctx().clone();
    let Some(pointer) = ctx
        .input(|i| i.pointer.hover_pos())
        .filter(|p| rect.contains(*p))
    else {
        return;
    };
    let left = pointer.x < rect.left() + 6.0;
    let right = pointer.x > rect.right() - 6.0;
    let top = pointer.y < rect.top() + 6.0;
    let bottom = pointer.y > rect.bottom() - 6.0;
    let (direction, cursor) = match (left, right, top, bottom) {
        (true, _, true, _) => (NorthWest, CursorIcon::ResizeNwSe),
        (_, true, _, true) => (SouthEast, CursorIcon::ResizeNwSe),
        (true, _, _, true) => (SouthWest, CursorIcon::ResizeNeSw),
        (_, true, true, _) => (NorthEast, CursorIcon::ResizeNeSw),
        (true, _, _, _) => (West, CursorIcon::ResizeHorizontal),
        (_, true, _, _) => (East, CursorIcon::ResizeHorizontal),
        (_, _, true, _) => (North, CursorIcon::ResizeVertical),
        (_, _, _, true) => (South, CursorIcon::ResizeVertical),
        _ => return,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}
