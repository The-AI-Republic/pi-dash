#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Url, WebviewUrl, WebviewWindowBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target_url = option_env!("PI_DASH_URL").unwrap_or("http://localhost:3000");
    eprintln!("Pi Dash: loading {target_url}");
    let parsed = Url::parse(target_url)?;

    tauri::Builder::default()
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(parsed))
                .title("Pi Dash")
                .inner_size(1400.0, 900.0)
                .min_inner_size(800.0, 600.0)
                .resizable(true)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())?;

    Ok(())
}
