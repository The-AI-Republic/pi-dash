//! PIDESKAPP-152: on Windows the pipe namespace is machine-wide, so another
//! local account can create `\\.\pipe\pidash-<id>` before our daemon does.
//! The client must refuse a pipe it did not get from its own user instead of
//! sending chat traffic to whoever holds the name.
#![cfg(windows)]

use std::ffi::c_void;
use std::time::Duration;

use pidash_ipc::{Client, Request, windows_pipe_name};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::windows::named_pipe::ServerOptions;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

const ERROR_INVALID_OWNER: i32 = 1307;

/// Create a pipe at `name` the way a hostile account would: owned by some
/// principal other than the current user, open to everyone. A single CI
/// account cannot own an object as a *different user*, but an elevated
/// token (GitHub's Windows runners) can name BUILTIN\Administrators as the
/// owner, which models "not me" for the client's check.
fn squat(name: &str) -> Option<tokio::net::windows::named_pipe::NamedPipeServer> {
    let sddl: Vec<u16> = "O:BAD:(A;;GA;;;WD)\0".encode_utf16().collect();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        )
    };
    assert!(ok != 0, "SDDL parse: {}", std::io::Error::last_os_error());
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    let created = unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .create_with_security_attributes_raw(name, &mut sa as *mut _ as *mut c_void)
    };
    unsafe { LocalFree(sd) };
    match created {
        Ok(p) => Some(p),
        Err(e) if e.raw_os_error() == Some(ERROR_INVALID_OWNER) => {
            eprintln!("skipping: token cannot assign a foreign owner (not elevated)");
            None
        }
        Err(e) => panic!("creating squatter pipe: {e}"),
    }
}

#[tokio::test]
async fn client_refuses_pipe_owned_by_another_principal() {
    let dir = std::env::temp_dir().join(format!("pidash-squat-{}", uuid::Uuid::new_v4()));
    let sock = dir.join("pidash.sock");
    let name = windows_pipe_name(&sock);
    let Some(squatter) = squat(&name) else { return };

    let received = tokio::spawn(async move {
        squatter.connect().await.ok()?;
        let mut line = String::new();
        BufReader::new(squatter).read_line(&mut line).await.ok()?;
        Some(line)
    });

    match Client::connect(&sock).await {
        Err(e) => {
            let msg = format!("{e:#}");
            assert!(msg.contains("not owned by the current user"), "{msg}");
            received.abort();
        }
        Ok(mut client) => {
            let _ = tokio::time::timeout(Duration::from_secs(2), client.call(Request::Hello)).await;
            let leaked = tokio::time::timeout(Duration::from_secs(2), received)
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten();
            panic!(
                "client connected to a pipe owned by another principal; \
                 the squatter received: {leaked:?}"
            );
        }
    }
}

#[tokio::test]
async fn client_accepts_the_owner_only_daemon_pipe() {
    let dir = std::env::temp_dir().join(format!("pidash-own-{}", uuid::Uuid::new_v4()));
    let sock = dir.join("pidash.sock");
    let name = windows_pipe_name(&sock);
    let server = pidash_ipc::windows_pipe::create_owner_only_pipe(&name, true).unwrap();
    let served = tokio::spawn(async move {
        server.connect().await.unwrap();
        let mut line = String::new();
        BufReader::new(server).read_line(&mut line).await.unwrap();
        line
    });
    let mut client = Client::connect(&sock).await.expect("own daemon pipe is accepted");
    let _ = tokio::time::timeout(Duration::from_millis(500), client.call(Request::Hello)).await;
    let line = tokio::time::timeout(Duration::from_secs(5), served).await.unwrap().unwrap();
    assert!(line.contains("hello"), "{line}");
}
