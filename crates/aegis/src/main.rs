//! main, event loop, and wiring for Aegis Browser
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod app;

use std::error::Error;
use std::fmt;

use aegis_engine::Start;
use eframe::egui;

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

fn start_from_args() -> Result<Start, StartupError> {
    let mut url = None;
    let mut test_page = false;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => {
                let value = args
                    .next()
                    .ok_or_else(|| StartupError::Arguments("--url needs an address".to_owned()))?;
                url = Some(value);
            }
            "--test-page" => test_page = true,
            _ => return Err(StartupError::Arguments(format!("unknown argument: {arg}"))),
        }
    }

    if test_page && url.is_some() {
        return Err(StartupError::Arguments(
            "use either --url or --test-page".to_owned(),
        ));
    }

    if test_page {
        return Ok(Start::Html(include_str!("test_page.html").to_owned()));
    }

    Ok(Start::Url(
        url.unwrap_or_else(|| "https://example.com".to_owned()),
    ))
}

fn main() -> Result<(), StartupError> {
    let start = start_from_args()?;

    #[cfg(target_os = "linux")]
    {
        // gtk and winit must both use x11 for child webviews
        gtk::gdk::set_allowed_backends("x11");
        gtk::init().map_err(|error| StartupError::Gtk(error.to_string()))?;
    }

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    let options = {
        use eframe::egui_glow::egui_winit::winit::platform::x11::EventLoopBuilderExtX11 as _;

        let mut options = options;
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
        options
    };

    eframe::run_native(
        "Aegis Browser",
        options,
        Box::new(|_| Ok(Box::new(app::AegisApp::new(start)))),
    )
    .map_err(StartupError::Eframe)
}
