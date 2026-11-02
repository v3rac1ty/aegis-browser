//! main, event loop, and wiring for Aegis Browser
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod accent_picker;
mod app;
mod command_bar;
mod compact;
#[cfg(debug_assertions)]
mod debug_shot;
mod favicons;
mod fonts;
mod glass;
mod icons;
mod imports;
mod logo;
mod matcher;
mod motion;
mod passwords;
mod paths;
mod persist;
mod preview;
mod protections;
mod security;
mod settings_page;
mod sidebar;
mod tabstrip;
mod theme;
mod views;
mod welcome;
mod widgets;

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use aegis_engine::Start;
use eframe::egui;

fn saved_vsync(data_dir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(data_dir.join("session.json")) else {
        return false;
    };
    let Ok(session) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    session
        .pointer("/browser/settings/frame_limit")
        .and_then(serde_json::Value::as_str)
        == Some("Vsync")
}

#[derive(Debug)]
enum StartupError {
    Arguments(String),
    #[cfg(target_os = "linux")]
    Gtk(String),
    Eframe(eframe::Error),
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(message) => write!(f, "{message}"),
            #[cfg(target_os = "linux")]
            Self::Gtk(message) => write!(f, "GTK initialization failed: {message}"),
            Self::Eframe(error) => write!(f, "window error: {error}"),
        }
    }
}

impl Error for StartupError {}

struct StartupOptions {
    start: Option<Start>,
    data_dir: PathBuf,
}

fn parse_args(args: &[String], current_dir: &Path) -> Result<StartupOptions, StartupError> {
    let mut url = None;
    let mut test_page = false;
    let mut data_dir = None;
    let mut args = args.iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => {
                let value = args
                    .next()
                    .ok_or_else(|| StartupError::Arguments("--url needs an address".to_owned()))?;
                url = Some(value.clone());
            }
            "--test-page" => test_page = true,
            "--data-dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| StartupError::Arguments("--data-dir needs a path".to_owned()))?;
                let path = PathBuf::from(value);
                data_dir = Some(if path.is_absolute() {
                    path
                } else {
                    current_dir.join(path)
                });
            }
            _ => return Err(StartupError::Arguments(format!("unknown argument: {arg}"))),
        }
    }

    if test_page && url.is_some() {
        return Err(StartupError::Arguments(
            "use either --url or --test-page".to_owned(),
        ));
    }

    let start = if test_page {
        Some(Start::Html(include_str!("test_page.html").to_owned()))
    } else {
        match url {
            Some(url) => {
                let page = aegis_ipc::page_url(&url)
                    .map_err(|error| StartupError::Arguments(format!("invalid --url: {error}")))?;
                Some(Start::Url(page.into_string()))
            }
            None => None,
        }
    };
    Ok(StartupOptions {
        start,
        data_dir: data_dir.unwrap_or_else(paths::data_dir),
    })
}

// debug size override, and an off-screen unfocused window when taking a screenshot
#[cfg(debug_assertions)]
fn debug_viewport(mut viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if let Some(size) = std::env::var("AEGIS_DEBUG_SIZE")
        .ok()
        .and_then(|value| debug_shot::parse_size(&value))
    {
        viewport = viewport.with_inner_size(size);
    }
    if std::env::var_os("AEGIS_DEBUG_SHOT").is_some() {
        viewport = viewport.with_position([-3000.0, 60.0]).with_active(false);
    }
    viewport
}

fn main() -> Result<(), StartupError> {
    let current_dir = std::env::current_dir()
        .map_err(|error| StartupError::Arguments(format!("current directory: {error}")))?;
    let options_from_args =
        parse_args(&std::env::args().skip(1).collect::<Vec<_>>(), &current_dir)?;

    #[cfg(target_os = "linux")]
    {
        // gtk and winit must both use x11 for child webviews
        gtk::gdk::set_allowed_backends("x11");
        gtk::init().map_err(|error| StartupError::Gtk(error.to_string()))?;
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_icon(logo::icon());
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        viewport = viewport.with_transparent(true);
    }
    #[cfg(target_os = "macos")]
    {
        // eframe maps titlebar_shown(false) to winit's transparent titlebar
        viewport = viewport
            .with_decorations(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_titlebar_buttons_shown(true)
            .with_title_shown(false);
    }
    #[cfg(debug_assertions)]
    {
        viewport = debug_viewport(viewport);
    }
    let mut options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport,
        ..Default::default()
    };
    options.glow_options.vsync = saved_vsync(&options_from_args.data_dir);

    #[cfg(target_os = "linux")]
    let options = {
        use winit::platform::x11::EventLoopBuilderExtX11 as _;

        let mut options = options;
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
        options
    };

    eframe::run_native(
        "Aegis Browser",
        options,
        Box::new(|creation| {
            Ok(Box::new(app::AegisApp::new(
                options_from_args.start,
                options_from_args.data_dir,
                creation.egui_ctx.clone(),
            )))
        }),
    )
    .map_err(StartupError::Eframe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_argument_resolves_relative_paths() {
        let current_dir = Path::new("/test/current");
        let args = vec!["--data-dir".to_owned(), "benchmark".to_owned()];
        let Ok(options) = parse_args(&args, current_dir) else {
            panic!("valid data directory argument should parse");
        };
        assert_eq!(options.data_dir, current_dir.join("benchmark"));
    }

    #[test]
    fn data_dir_argument_requires_value() {
        let args = vec!["--data-dir".to_owned()];
        assert!(matches!(
            parse_args(&args, Path::new("/test/current")),
            Err(StartupError::Arguments(message)) if message == "--data-dir needs a path"
        ));
    }
}
