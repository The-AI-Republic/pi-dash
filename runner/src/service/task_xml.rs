//! Per-user scheduled task definition (Windows Task Scheduler XML).
//!
//! Lives outside the `cfg(windows)` module on purpose: rendering is a pure
//! string template with no OS dependency, so its contract tests run on every
//! platform's unit-test suite instead of only compiling unexecuted on
//! Windows (where CI never runs the runner's lib tests).

/// Render the per-user scheduled task definition (Task Scheduler XML).
/// Pure so the restart block is unit-testable without `schtasks.exe`.
///
/// `user_sid` scopes the logon trigger and principal to the installing
/// user: an unscoped `<LogonTrigger>` fires at *any* account's logon.
/// The XML declaration says UTF-8 because `install_via_xml` writes UTF-8
/// bytes; a UTF-16 declaration on UTF-8 content makes schtasks reject it.
pub fn render_task_xml(exe: &str, user_sid: &str) -> String {
    let exe_escaped = xml_escape(exe);
    let user_escaped = xml_escape(user_sid);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Pi Dash Runner daemon (starts at logon, restarts on failure)</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user_escaped}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal>
      <UserId>{user_escaped}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <AllowStartOnDemand>true</AllowStartOnDemand>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe_escaped}" __run</Command>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::{render_task_xml, xml_escape};

    #[test]
    fn task_xml_has_restart_on_failure_and_logon_trigger() {
        let xml = render_task_xml("C:\\Program Files\\Pi Dash\\pidash.exe", "S-1-5-21-1001");
        assert!(
            xml.contains("<RestartOnFailure>"),
            "must contain RestartOnFailure: {xml}"
        );
        assert!(
            xml.contains("<Interval>PT1M</Interval>"),
            "interval PT1M: {xml}"
        );
        assert!(xml.contains("<Count>3</Count>"), "count 3: {xml}");
        assert!(xml.contains("<LogonTrigger>"), "logon trigger: {xml}");
        assert!(xml.contains("pidash.exe"), "exe path: {xml}");
        // Must not use plain schtasks ONLOGON-only fallback as primary.
        assert!(
            xml.contains("InteractiveToken"),
            "per-user interactive token: {xml}"
        );
        // Logon trigger and principal must be scoped to the installing
        // user, not fire at any account's logon.
        assert!(
            xml.contains("<UserId>S-1-5-21-1001</UserId>"),
            "user-scoped logon: {xml}"
        );
        // The file is written as UTF-8 (tokio::fs::write on a String), so
        // the declaration must say UTF-8 or schtasks rejects it.
        assert!(
            xml.contains("encoding=\"UTF-8\""),
            "utf-8 declaration: {xml}"
        );
    }

    #[test]
    fn xml_escape_prevents_breakout() {
        let escaped = xml_escape(r#"C:\a&b<>"'"#);
        assert_eq!(escaped, "C:\\a&amp;b&lt;&gt;&quot;&apos;");
    }
}
