/// Represents an opcode as defined by the WebSocket protocol.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Opcode {
    /// UTF-8 text.
    Text,
    /// Arbitrary binary data.
    Binary,
    /// Close control frame.
    Close,
    /// Ping control frame.
    Ping,
    /// Pong control frame.
    Pong,
}

impl Opcode {
    /// Returns `true` if `self` is `Text`.
    #[must_use]
    pub fn is_text(self) -> bool {
        matches!(self, Self::Text)
    }

    /// Returns `true` if `self` is `Close`, `Ping` or `Pong`.
    #[must_use]
    pub fn is_control(self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong)
    }

    /// Converts `u8` to `Opcode`.
    ///
    /// Returns `None` for unrecognised and unsupported opcodes.
    #[must_use]
    pub fn try_from(data: u8) -> Option<Self> {
        let opcode = match data {
            1 => Self::Text,
            2 => Self::Binary,
            8 => Self::Close,
            9 => Self::Ping,
            10 => Self::Pong,
            _ => {
                return None;
            }
        };

        Some(opcode)
    }
}

impl From<Opcode> for u8 {
    fn from(opcode: Opcode) -> Self {
        match opcode {
            Opcode::Text => 1,
            Opcode::Binary => 2,
            Opcode::Close => 8,
            Opcode::Ping => 9,
            Opcode::Pong => 10,
        }
    }
}
