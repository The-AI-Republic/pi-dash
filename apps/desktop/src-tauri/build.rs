fn main() {
    println!("cargo:rerun-if-env-changed=PI_DASH_URL");

    if let Ok(url) = std::env::var("PI_DASH_URL")
        && url::Url::parse(&url).is_err()
    {
        panic!("PI_DASH_URL is set but is not a valid absolute URL: {url}");
    }

    tauri_build::build()
}
