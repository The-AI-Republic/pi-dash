fn main() {
    println!("cargo:rerun-if-env-changed=PI_DASH_URL");
    tauri_build::build()
}
