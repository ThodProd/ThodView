#![windows_subsystem = "windows"]

mod decode;
mod metadata;
mod ocr;
mod process;
mod shell;
mod ui;
mod view;

use eframe::egui::{IconData, ViewportBuilder};
use std::{ffi::OsStr, path::PathBuf};

fn main() {
    let first = std::env::args_os().nth(1);
    let hub_hook = matches!(
        first.as_deref().and_then(OsStr::to_str),
        Some("--register-integrations" | "--uninstall-context" | "--default-viewer")
    );
    if let Err(error) = run(first) {
        let message = error.to_string();
        eprintln!("{message}");
        if !hub_hook {
            rfd::MessageDialog::new()
                .set_title("ThodView")
                .set_description(message)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        std::process::exit(1);
    }
}

fn run(first: Option<std::ffi::OsString>) -> process::Result<()> {
    let settings = ui::Settings::load();
    match first.as_deref().and_then(OsStr::to_str) {
        Some("--register-integrations") => return shell::register(settings.context_menu, true),
        Some("--default-viewer") => {
            shell::register(settings.context_menu, true)?;
            return shell::defaults();
        }
        Some("--uninstall-context") => return shell::unregister(),
        _ => {}
    }

    let path = if first.as_deref() == Some(OsStr::new("--")) {
        std::env::args_os().nth(2)
    } else {
        first
    }
    .map(PathBuf::from);
    let packaged = std::env::current_exe()?
        .with_file_name("runtime")
        .join("ffmpeg/ffmpeg.exe")
        .is_file();
    let integration_error = if packaged && !shell::registered_installation_exists() {
        shell::register(settings.context_menu, true)
            .err()
            .map(|err| err.to_string())
    } else {
        None
    };
    let mut viewport = ViewportBuilder::default()
        .with_title("ThodView")
        .with_app_id("ThodView")
        .with_inner_size([
            settings.width.clamp(620.0, 3840.0),
            settings.height.clamp(420.0, 2160.0),
        ])
        .with_min_inner_size([620.0, 420.0])
        .with_decorations(false)
        .with_resizable(true)
        .with_drag_and_drop(true)
        .with_maximized(settings.maximized);
    if let Ok(image) = image::load_from_memory_with_format(
        include_bytes!("../assets/thodview.ico"),
        image::ImageFormat::Ico,
    ) {
        let image = image.into_rgba8();
        viewport = viewport.with_icon(IconData {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        });
    }
    let native = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "ThodView",
        native,
        Box::new(|context| {
            Ok(Box::new(ui::Viewer::new(
                context,
                settings,
                path,
                integration_error,
            )))
        }),
    )
    .map_err(|err| err.to_string().into())
}
