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
    pub fn is_text(self) -> bool {
        match self {
            Opcode::Text => true,
            _ => false,
        }
    }

    /// Returns `true` if `self` is `Close`, `Ping` or `Pong`.
    pub fn is_control(self) -> bool {
        match self {
            Opcode::Text | Opcode::Binary => false,
            _ => true,
        }
    }

    /// Converts `u8` to `Opcode`.
    ///
    /// Returns `None` for unrecognised and unsupported opcodes.
    pub fn try_from(data: u8) -> Option<Self> {
        let opcode = match data {
            1 => Opcode::Text,
            2 => Opcode::Binary,
            8 => Opcode::Close,
            9 => Opcode::Ping,
            10 => Opcode::Pong,
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
