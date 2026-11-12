//! Records the target triple this `pidash` was built for, so the binary can
//! name its own release archive (`pidash-<target>.tar.xz` / `.zip`) when it
//! installs the built-in engine that ships in that archive
//! (`src/engine/install.rs`). A musl build must fetch the musl archive, which
//! no runtime probe of the host can tell apart from the glibc one.

fn main() {
    let target = std::env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=PIDASH_BUILD_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
