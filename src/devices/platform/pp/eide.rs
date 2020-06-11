use crate::devices::prelude::*;

use crate::devices::generic::ide::{IdeController, IdeIdx, IdeReg};
use crate::signal::irq;

#[derive(Debug, Default)]
struct IdeDriveCfg {
    primary_timing: [u32; 2],
    secondary_timing: [u32; 2],
    config: u32,
    controller_status: u32,
}

/// PP5020 EIDE Controller
#[derive(Debug)]
pub struct EIDECon {
    ide0_cfg: IdeDriveCfg,
    ide1_cfg: IdeDriveCfg,
    ide: IdeController,

    // not sure if these are here, or under the generic IDE interface. we'll find out when I get
    // around to implementing DMA I guess ¯\_(ツ)_/¯
    dma_control: u32,
    dma_length: u32,
    dma_addr: u32,
    unknown: u32,
}

impl EIDECon {
    pub fn new(irq: irq::Sender) -> EIDECon {
        EIDECon {
            ide0_cfg: Default::default(),
            ide1_cfg: Default::default(),
            ide: IdeController::new(irq),

            dma_control: 0,
            dma_length: 0,
            dma_addr: 0,
            unknown: 0,
        }
    }

    pub fn as_ide(&mut self) -> &mut IdeController {
        &mut self.ide
    }
}

impl Device for EIDECon {
    fn kind(&self) -> &'static str {
        "EIDE Controller"
    }

    fn probe(&self, offset: u32) -> Probe {
        let reg = match offset {
            0x000 => "IDE0 Primary Timing 0",
            0x004 => "IDE0 Primary Timing 1",
            0x008 => "IDE0 Secondary Timing 0",
            0x00c => "IDE0 Secondary Timing 1",
            0x010 => "IDE1 Primary Timing 0",
            0x014 => "IDE1 Primary Timing 1",
            0x018 => "IDE1 Secondary Timing 0",
            0x01c => "IDE1 Secondary Timing 1",
            0x028 => "IDE0 Cfg",
            0x02c => "IDE1 Cfg",

            0x1e0 => "Data",
            0x1e4 => "Error/Features",
            0x1e8 => "Sector Count",
            0x1ec => "Sector Number",
            0x1f0 => "Cylinder Low",
            0x1f4 => "Cylinder High",
            0x1f8 => "Device Head",
            0x1fc => "Status/Command",

            0x3f8 => "AltStatus/DeviceControl",
            0x3fc => "Data Latch",

            0x400 => "DMA Control",
            0x408 => "DMA Length",
            0x40c => "DMA Addr",
            0x410 => "?",

            _ => return Probe::Unmapped,
        };

        Probe::Register(reg)
    }
}

impl Memory for EIDECon {
    fn r32(&mut self, offset: u32) -> MemResult<u32> {
        match offset {
            0x000 => Ok(self.ide0_cfg.primary_timing[0]),
            0x004 => Ok(self.ide0_cfg.primary_timing[1]),
            0x008 => Ok(self.ide0_cfg.secondary_timing[0]),
            0x00c => Ok(self.ide0_cfg.secondary_timing[1]),
            0x010 => Ok(self.ide1_cfg.primary_timing[0]),
            0x014 => Ok(self.ide1_cfg.primary_timing[1]),
            0x018 => Ok(self.ide1_cfg.secondary_timing[0]),
            0x01c => Ok(self.ide1_cfg.secondary_timing[1]),
            0x028 => {
                let val = *0u32
                    .set_bit(4, self.ide.irq_state(IdeIdx::IDE0))
                    .set_bit(5, self.ide.irq_state(IdeIdx::IDE1));
                Err(StubRead(Info, val))
            }
            0x02c => Err(Unimplemented),

            0x1e0 => self.ide.read16(IdeReg::Data).map(|v| v as u32),
            0x1e4 => self.ide.read8(IdeReg::Error).map(|v| v as u32),
            0x1e8 => self.ide.read8(IdeReg::SectorCount).map(|v| v as u32),
            0x1ec => self.ide.read8(IdeReg::SectorNo).map(|v| v as u32),
            0x1f0 => self.ide.read8(IdeReg::CylinderLo).map(|v| v as u32),
            0x1f4 => self.ide.read8(IdeReg::CylinderHi).map(|v| v as u32),
            0x1f8 => self.ide.read8(IdeReg::DeviceHead).map(|v| v as u32),
            0x1fc => self.ide.read8(IdeReg::Status).map(|v| v as u32),

            0x3f8 => self.ide.read8(IdeReg::AltStatus).map(|v| v as u32),
            0x3fc => self.ide.read8(IdeReg::DataLatch).map(|v| v as u32),

            0x400 => Err(StubRead(Error, self.dma_control)),
            0x408 => Err(StubRead(Error, self.dma_length)),
            0x40c => Err(StubRead(Error, self.dma_addr)),
            0x410 => Err(StubRead(Error, self.unknown)),
            _ => Err(Unexpected),
        }
    }

    fn w32(&mut self, offset: u32, val: u32) -> MemResult<()> {
        match offset {
            0x000 => Ok(self.ide0_cfg.primary_timing[0] = val),
            0x004 => Ok(self.ide0_cfg.primary_timing[1] = val),
            0x008 => Ok(self.ide0_cfg.secondary_timing[0] = val),
            0x00c => Ok(self.ide0_cfg.secondary_timing[1] = val),
            0x010 => Ok(self.ide1_cfg.primary_timing[0] = val),
            0x014 => Ok(self.ide1_cfg.primary_timing[1] = val),
            0x018 => Ok(self.ide1_cfg.secondary_timing[0] = val),
            0x01c => Ok(self.ide1_cfg.secondary_timing[1] = val),
            0x028 => {
                if val.get_bit(4) {
                    self.ide.clear_irq(IdeIdx::IDE0)
                }
                if val.get_bit(5) {
                    self.ide.clear_irq(IdeIdx::IDE1)
                }
                Err(StubWrite(Info, ()))
            }
            0x02c => Err(Unimplemented),

            0x1e0 => self.ide.write16(IdeReg::Data, val as u16),
            0x1e4 => self.ide.write8(IdeReg::Features, val as u8),
            0x1e8 => self.ide.write8(IdeReg::SectorCount, val as u8),
            0x1ec => self.ide.write8(IdeReg::SectorNo, val as u8),
            0x1f0 => self.ide.write8(IdeReg::CylinderLo, val as u8),
            0x1f4 => self.ide.write8(IdeReg::CylinderHi, val as u8),
            0x1f8 => self.ide.write8(IdeReg::DeviceHead, val as u8),
            0x1fc => self.ide.write8(IdeReg::Command, val as u8),

            0x3f8 => self.ide.write8(IdeReg::DevControl, val as u8),
            0x3fc => self.ide.write8(IdeReg::DataLatch, val as u8),

            0x400 => Err(StubWrite(Error, self.dma_control = val)),
            0x408 => Err(StubWrite(Error, self.dma_length = val)),
            0x40c => Err(StubWrite(Error, self.dma_addr = val)),
            0x410 => Err(StubWrite(Error, self.unknown = val)),

            _ => Err(Unexpected),
        }
    }
}
