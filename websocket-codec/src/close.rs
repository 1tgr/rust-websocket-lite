use std::{fmt, str};

use bytes::Bytes;

/// Status code used to indicate why an endpoint is closing the WebSocket connection.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum CloseCode {
    /// Indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    Normal,
    /// Indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    Away,
    /// Indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    Protocol,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    Unsupported,
    /// Indicates that no status code was included in a closing frame. This
    /// close code makes it possible to use a single method, `on_close` to
    /// handle even cases where no close code was provided.
    Status,
    /// Indicates an abnormal closure. If the abnormal closure was due to an
    /// error, this close code will not be used. Instead, the `on_error` method
    /// of the handler will be called with the error. However, if the connection
    /// is simply dropped, without an error, this close code will be sent to the
    /// handler.
    Abnormal,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 \[RFC3629\]
    /// data within a text message).
    Invalid,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., Unsupported or Size) or if there
    /// is a need to hide specific details about the policy.
    Policy,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    Size,
    /// Indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the WebSocket handshake.  The list of extensions that
    /// are needed should be given as the reason for closing.
    /// Note that this status code is not used by the server, because it
    /// can fail the WebSocket handshake instead.
    Extension,
    /// Indicates that a server is terminating the connection because
    /// it encountered an unexpected condition that prevented it from
    /// fulfilling the request.
    Error,
    /// Indicates that the server is restarting. A client may choose to reconnect,
    /// and if it does, it should use a randomized delay of 5-30 seconds between attempts.
    Restart,
    /// Indicates that the server is overloaded and the client should either connect
    /// to a different IP (when multiple targets exist), or reconnect to the same IP
    /// when a user has performed an action.
    Again,
    #[doc(hidden)]
    Tls,
    #[doc(hidden)]
    Reserved(u16),
    #[doc(hidden)]
    Iana(u16),
    #[doc(hidden)]
    Library(u16),
    #[doc(hidden)]
    Bad(u16),
}

impl CloseCode {
    /// Check if this `CloseCode` is allowed.
    #[must_use]
    pub fn is_allowed(self) -> bool {
        !matches!(
            self,
            Self::Bad(_) | Self::Reserved(_) | Self::Status | Self::Abnormal | Self::Tls
        )
    }
}

impl fmt::Display for CloseCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code: u16 = self.into();
        f.write_str(&code.to_string())
    }
}

impl From<CloseCode> for u16 {
    fn from(code: CloseCode) -> u16 {
        match code {
            CloseCode::Normal => 1000,
            CloseCode::Away => 1001,
            CloseCode::Protocol => 1002,
            CloseCode::Unsupported => 1003,
            CloseCode::Status => 1005,
            CloseCode::Abnormal => 1006,
            CloseCode::Invalid => 1007,
            CloseCode::Policy => 1008,
            CloseCode::Size => 1009,
            CloseCode::Extension => 1010,
            CloseCode::Error => 1011,
            CloseCode::Restart => 1012,
            CloseCode::Again => 1013,
            CloseCode::Tls => 1015,
            CloseCode::Reserved(code) | CloseCode::Iana(code) | CloseCode::Library(code) | CloseCode::Bad(code) => code,
        }
    }
}

impl<'t> From<&'t CloseCode> for u16 {
    fn from(code: &'t CloseCode) -> u16 {
        (*code).into()
    }
}

impl From<u16> for CloseCode {
    fn from(code: u16) -> CloseCode {
        match code {
            1000 => CloseCode::Normal,
            1001 => CloseCode::Away,
            1002 => CloseCode::Protocol,
            1003 => CloseCode::Unsupported,
            1005 => CloseCode::Status,
            1006 => CloseCode::Abnormal,
            1007 => CloseCode::Invalid,
            1008 => CloseCode::Policy,
            1009 => CloseCode::Size,
            1010 => CloseCode::Extension,
            1011 => CloseCode::Error,
            1012 => CloseCode::Restart,
            1013 => CloseCode::Again,
            1015 => CloseCode::Tls,
            1016..=2999 => CloseCode::Reserved(code),
            3000..=3999 => CloseCode::Iana(code),
            4000..=4999 => CloseCode::Library(code),
            _ => CloseCode::Bad(code),
        }
    }
}

/// Represents the reason assigned to a message with the Close opcode.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CloseFrame {
    pub(crate) code: CloseCode,
    pub(crate) reason: Bytes,
}

impl CloseFrame {
    /// Returns the reason as a code.
    pub fn code(&self) -> CloseCode {
        self.code
    }

    /// Returns the reason as text string.
    pub fn reason(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.reason) }
    }
}
