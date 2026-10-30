use serde::Serialize;
use std::fmt;

/// What kind of thing went wrong, so the UI can offer the right fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// The server wants a password or a key we don't have.
    Auth,
    /// The server's fingerprint isn't in known_hosts yet.
    HostKey,
    /// Couldn't reach the server at all.
    Network,
    NotFound,
    Exists,
    TooLarge,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpError {
    pub code: ErrorCode,
    pub message: String,
}

impl OpError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Failed, message)
    }

    /// Turns a failed `ssh` run into something a person can act on.
    pub fn from_ssh(exit_code: Option<i32>, stderr: &str) -> Self {
        let last = crate::text::last_meaningful_line(stderr);
        if exit_code == Some(255) {
            if stderr.contains("Permission denied") || stderr.contains("Too many authentication failures") {
                return Self::new(
                    ErrorCode::Auth,
                    "This server needs you to sign in. Open a terminal to it once, then try again.",
                );
            }
            if stderr.contains("Host key verification failed") || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
                return Self::new(
                    ErrorCode::HostKey,
                    "This server isn't trusted yet. Open a terminal to it once to confirm its fingerprint.",
                );
            }
            let detail = if last.is_empty() { "connection failed".to_string() } else { last };
            return Self::new(ErrorCode::Network, format!("Can't reach the server: {detail}"));
        }
        if stderr.contains("No such file or directory") {
            return Self::new(ErrorCode::NotFound, "That file or folder doesn't exist anymore.");
        }
        if last.is_empty() {
            Self::failed(format!("The command failed (exit code {})", exit_code.unwrap_or(-1)))
        } else {
            Self::failed(last)
        }
    }
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for OpError {}

impl From<std::io::Error> for OpError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Self::new(ErrorCode::NotFound, e.to_string()),
            std::io::ErrorKind::AlreadyExists => Self::new(ErrorCode::Exists, e.to_string()),
            _ => Self::failed(e.to_string()),
        }
    }
}
