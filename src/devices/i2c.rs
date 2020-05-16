use crate::devices::{Device, Probe};
use crate::memory::{MemException::*, MemResult, Memory};

/// I2C Controller
#[derive(Debug)]
pub struct I2CCon {
    // TODO
}

impl I2CCon {
    pub fn new_hle() -> I2CCon {
        I2CCon {}
    }
}

impl Device for I2CCon {
    fn kind(&self) -> &'static str {
        "I2CCon"
    }

    fn probe(&self, offset: u32) -> Probe<'_> {
        let reg = match offset {
            0x00c => "Data0 (?)",
            0x100 => "?",
            0x104 => "?",
            0x120 => "?",
            0x140 => "Scroll Wheel + Keypad Buttons",
            _ => return Probe::Unmapped,
        };

        Probe::Register(reg)
    }
}

impl Memory for I2CCon {
    fn r32(&mut self, offset: u32) -> MemResult<u32> {
        match offset {
            0x00c => Err(StubRead(0x00000000)),
            0x100 => Err(StubRead(0x00000000)),
            0x104 => Err(StubRead(0x00000000)),
            0x120 => Err(StubRead(0x00000000)),
            0x140 => Err(StubRead(0x00000000)),
            _ => Err(Unexpected),
        }
    }

    fn w32(&mut self, offset: u32, val: u32) -> MemResult<()> {
        let _ = val;

        match offset {
            0x00c => Err(StubWrite)?,
            0x100 => Err(StubWrite)?,
            0x104 => Err(StubWrite)?,
            0x120 => Err(StubWrite)?,
            0x140 => Err(StubWrite)?,
            _ => return Err(Unexpected),
        }

        Ok(())
    }
}
