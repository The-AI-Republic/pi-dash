pub mod client;
pub mod protocol;
pub mod server;

#[cfg(windows)]
pub(crate) fn windows_pipe_name(path: &std::path::Path) -> String {
    let id = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        path.to_string_lossy().as_bytes(),
    );
    format!(r"\\.\pipe\pidash-{id}")
}
