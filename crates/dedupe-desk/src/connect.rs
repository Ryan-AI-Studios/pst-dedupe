//! Connect dialog state + Solo/Connected mode guards (track **0064**).

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use eframe::egui;
use zeroize::Zeroize;

use crate::remote_client::{
    normalize_base_url, ConnectedSession, RemoteClient, RemoteError, SecretString, DEFAULT_BASE_URL,
};

/// Result of a background Connect (password or SSO exchange) attempt.
#[derive(Debug)]
pub enum ConnectAttemptResult {
    Ok(ConnectedSession),
    Err(String),
    OidcRequired(String),
}

/// Connect dialog draft fields (password zeroized after attempt).
pub struct ConnectDialogState {
    pub open: bool,
    pub base_url: String,
    pub username: String,
    pub password: SecretString,
    pub tenant_slug: String,
    pub busy: bool,
    pub error: Option<String>,
    rx: Option<Receiver<ConnectAttemptResult>>,
    /// SSO loopback listener in progress (port for UI message).
    pub sso_pending_port: Option<u16>,
}

impl Default for ConnectDialogState {
    fn default() -> Self {
        Self {
            open: false,
            base_url: DEFAULT_BASE_URL.into(),
            username: String::new(),
            password: SecretString::default(),
            tenant_slug: String::new(),
            busy: false,
            error: None,
            rx: None,
            sso_pending_port: None,
        }
    }
}

impl ConnectDialogState {
    pub fn open_dialog(&mut self) {
        self.open = true;
        self.error = None;
        if self.base_url.trim().is_empty() {
            self.base_url = DEFAULT_BASE_URL.into();
        }
    }

    /// Dialog open and/or Connect worker in flight — blocks local Solo open/create.
    pub fn is_pending(&self) -> bool {
        self.open || self.busy || self.rx.is_some()
    }

    pub fn close(&mut self) {
        // Unblock SSO loopback listener promptly (P3 cancel hygiene).
        if let Some(port) = self.sso_pending_port.take() {
            let _ = std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                std::time::Duration::from_millis(200),
            );
        }
        self.open = false;
        self.busy = false;
        self.error = None;
        self.rx = None;
        self.password.clear();
    }

    /// Start password Connect on a background thread.
    pub fn start_password_connect(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let base = match normalize_base_url(&self.base_url) {
            Ok(b) => b,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        self.base_url = base.clone();
        let name = self.username.trim().to_string();
        if name.is_empty() {
            self.error = Some("Username is required.".into());
            return;
        }
        let mut password = std::mem::take(&mut self.password);
        let pass = password.as_str().to_string();
        password.clear();
        self.password = SecretString::default();

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.error = None;
        let ctx = ctx.clone();
        let _ = thread::Builder::new()
            .name("desk-connect".into())
            .spawn(move || {
                let result = connect_password_blocking(&base, &name, &pass);
                // Best-effort zeroize of password copy.
                let mut pass = pass;
                pass.zeroize();
                let _ = tx.send(result);
                ctx.request_repaint();
            });
    }

    /// Poll background Connect result.
    pub fn poll(&mut self) -> Option<ConnectAttemptResult> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(r) => {
                self.rx = None;
                self.busy = false;
                self.sso_pending_port = None;
                Some(r)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                self.busy = false;
                self.sso_pending_port = None;
                Some(ConnectAttemptResult::Err(
                    "Connect worker ended unexpectedly.".into(),
                ))
            }
        }
    }
}

fn connect_password_blocking(base: &str, name: &str, password: &str) -> ConnectAttemptResult {
    let client = match RemoteClient::new() {
        Ok(c) => c,
        Err(e) => return ConnectAttemptResult::Err(e),
    };
    if let Err(e) = client.healthz(base) {
        return ConnectAttemptResult::Err(e.to_string());
    }
    match client.login(base, name, password) {
        Ok(session) => ConnectAttemptResult::Ok(session),
        Err(RemoteError::Http {
            code,
            message,
            status,
            ..
        }) if code == "oidc_required" || status == 403 && message.contains("OIDC") => {
            ConnectAttemptResult::OidcRequired(message)
        }
        Err(e) if e.is_oidc_required() => ConnectAttemptResult::OidcRequired(e.to_string()),
        Err(e) => ConnectAttemptResult::Err(e.to_string()),
    }
}

/// Refuse local matter open while Connected **or** while Connect dialog/worker is pending.
pub fn can_open_local_matter(connected: bool, connect_pending: bool) -> Result<(), String> {
    if connected {
        Err("Disconnect from matter-service before opening a local matter.".into())
    } else if connect_pending {
        Err("Finish or cancel Connect before opening a local matter.".into())
    } else {
        Ok(())
    }
}

/// Refuse Connect while a local matter is open (operator must close first).
pub fn can_connect_with_local_matter(matter_open: bool) -> Result<(), String> {
    if matter_open {
        Err("Close the local matter before connecting to matter-service.".into())
    } else {
        Ok(())
    }
}

/// Refuse applying a successful Connect when a local matter is already open (fail closed).
pub fn can_apply_connect_session(matter_open: bool) -> Result<(), String> {
    if matter_open {
        Err("Connect refused: a local matter is open. Close the matter first, then Connect.".into())
    } else {
        Ok(())
    }
}

/// Refuse applying local open/create completion while Connected or Connect is pending.
pub fn can_apply_local_matter(connected: bool, connect_pending: bool) -> Result<(), String> {
    can_open_local_matter(connected, connect_pending)
}

/// Start SSO: bind ephemeral loopback, open browser to service OIDC login with handoff URL.
pub fn start_sso_connect(dialog: &mut ConnectDialogState, ctx: &egui::Context) {
    if dialog.busy {
        return;
    }
    let base = match normalize_base_url(&dialog.base_url) {
        Ok(b) => b,
        Err(e) => {
            dialog.error = Some(e);
            return;
        }
    };
    dialog.base_url = base.clone();
    let tenant = dialog.tenant_slug.trim().to_string();

    // Bind on the UI side so Cancel can self-connect to unblock the worker.
    let listener = match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            dialog.error = Some(format!("Failed to bind loopback for SSO: {e}"));
            return;
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(e) => {
            dialog.error = Some(format!("SSO listener: {e}"));
            return;
        }
    };

    let (tx, rx) = mpsc::channel();
    dialog.rx = Some(rx);
    dialog.busy = true;
    dialog.error = None;
    dialog.sso_pending_port = Some(port);
    let ctx = ctx.clone();
    let _ = thread::Builder::new()
        .name("desk-sso".into())
        .spawn(move || {
            let result = sso_loopback_blocking(&base, tenant.as_str(), listener, port);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
}

fn sso_loopback_blocking(
    base: &str,
    tenant: &str,
    listener: std::net::TcpListener,
    port: u16,
) -> ConnectAttemptResult {
    use std::io::{ErrorKind, Read, Write};
    use std::time::{Duration, Instant};

    let client = match RemoteClient::new() {
        Ok(c) => c,
        Err(e) => return ConnectAttemptResult::Err(e),
    };
    if let Err(e) = client.healthz(base) {
        return ConnectAttemptResult::Err(e.to_string());
    }

    let handoff = format!("http://127.0.0.1:{port}/connect/callback");
    if !crate::remote_client::is_loopback_handoff_url(&handoff) {
        return ConnectAttemptResult::Err("Internal: handoff URL failed loopback check.".into());
    }

    let mut login_url = format!(
        "{base}/v1/oidc/login?handoff_url={}",
        urlencoding_param(&handoff)
    );
    if !tenant.is_empty() {
        login_url.push_str(&format!("&tenant={}", urlencoding_param(tenant)));
    }

    if let Err(e) = open_system_browser(&login_url) {
        return ConnectAttemptResult::Err(format!("Could not open system browser: {e}"));
    }

    // Wait for one callback (up to 3 minutes) with nonblocking accept + poll.
    if let Err(e) = listener.set_nonblocking(true) {
        return ConnectAttemptResult::Err(format!("SSO listener nonblocking: {e}"));
    }
    let deadline = Instant::now() + Duration::from_secs(180);
    let stream = loop {
        if Instant::now() >= deadline {
            return ConnectAttemptResult::Err(
                "SSO timed out waiting for browser callback (3 minutes).".into(),
            );
        }
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => {
                return ConnectAttemptResult::Err(format!("SSO handoff wait failed: {e}"));
            }
        }
    };

    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let code = extract_query_param(&req, "code");
    // Always respond so the browser does not hang (real callbacks only).
    if !req.is_empty() {
        let body = if code.is_some() {
            "<html><body><p>Signed in — you can close this window and return to Dedupe Desk.</p></body></html>"
        } else {
            "<html><body><p>SSO callback missing code. You can close this window.</p></body></html>"
        };
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
    }
    let Some(code) = code else {
        // Cancel self-connect is a bare TCP probe (empty/non-GET); browser hits are GET.
        if req.is_empty() || !req.starts_with("GET ") {
            return ConnectAttemptResult::Err("SSO cancelled.".into());
        }
        return ConnectAttemptResult::Err("SSO callback did not include a one-time code.".into());
    };
    match client.exchange_oidc_code(base, &code) {
        Ok(session) => ConnectAttemptResult::Ok(session),
        Err(e) => ConnectAttemptResult::Err(format!("SSO exchange failed: {e}")),
    }
}

fn extract_query_param(http_req: &str, key: &str) -> Option<String> {
    let line = http_req.lines().next()?;
    // GET /connect/callback?code=... HTTP/1.1
    let path = line.split_whitespace().nth(1)?;
    let q = path.split('?').nth(1)?;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next()?;
        let v = it.next().unwrap_or("");
        if k == key {
            return Some(url_decode(v));
        }
    }
    None
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v as char);
                    i += 3;
                    continue;
                }
                out.push('%');
                i += 1;
            }
            b'+' => {
                out.push(' ');
                i += 1;
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn urlencoding_param(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn open_system_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        // CREATE_NO_WINDOW — avoid flashing a console.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        use std::process::Command;
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_guards_refuse_dual_open() {
        assert!(can_open_local_matter(true, false).is_err());
        assert!(can_open_local_matter(false, false).is_ok());
        assert!(can_connect_with_local_matter(true).is_err());
        assert!(can_connect_with_local_matter(false).is_ok());
    }

    #[test]
    fn connect_pending_blocks_local_open() {
        assert!(can_open_local_matter(false, true).is_err());
        assert!(can_apply_local_matter(false, true).is_err());
        let msg = can_open_local_matter(false, true).unwrap_err();
        assert!(msg.to_ascii_lowercase().contains("connect"));
    }

    #[test]
    fn apply_connect_refuses_when_matter_open() {
        assert!(can_apply_connect_session(true).is_err());
        assert!(can_apply_connect_session(false).is_ok());
        let msg = can_apply_connect_session(true).unwrap_err();
        assert!(msg.to_ascii_lowercase().contains("matter"));
    }

    #[test]
    fn apply_local_matter_refuses_when_connected() {
        assert!(can_apply_local_matter(true, false).is_err());
        assert!(can_apply_local_matter(false, false).is_ok());
    }

    #[test]
    fn connect_dialog_pending_covers_open_and_busy() {
        let mut d = ConnectDialogState::default();
        assert!(!d.is_pending());
        d.open = true;
        assert!(d.is_pending());
        d.open = false;
        d.busy = true;
        assert!(d.is_pending());
    }

    #[test]
    fn login_failure_leaves_solo_semantics() {
        // ConnectAttemptResult::Err must not imply a session exists.
        let r = ConnectAttemptResult::Err("bad password".into());
        match r {
            ConnectAttemptResult::Ok(_) => panic!("should stay Solo"),
            ConnectAttemptResult::Err(_) | ConnectAttemptResult::OidcRequired(_) => {}
        }
    }
}
