#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{WebviewUrl, WebviewWindowBuilder};

const DEFAULT_URL: &str = "http://localhost:3000";

fn main() {
    let target_url: &str = option_env!("PI_DASH_URL").unwrap_or(DEFAULT_URL);

    tauri::Builder::default()
        .setup(move |app| {
            let parsed = url::Url::parse(target_url)
                .expect("PI_DASH_URL must be a valid absolute URL");

            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(parsed))
                .title("Pi Dash")
                .inner_size(1400.0, 900.0)
                .min_inner_size(800.0, 600.0)
                .resizable(true)
                .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Pi Dash desktop");
}
