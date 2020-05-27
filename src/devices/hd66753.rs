use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

use bit_field::BitField;
use crossbeam_channel as chan;
use log::Level::*;
use minifb::{Key, Window, WindowOptions};

use crate::devices::{Device, Probe};
use crate::memory::{MemException::*, MemResult, Memory};

const CGRAM_WIDTH: usize = 168;
const CGRAM_HEIGHT: usize = 132;
#[allow(dead_code)]
const CGRAM_BYTES: usize = (CGRAM_WIDTH * CGRAM_HEIGHT) * 2 / 8; // 168 * 132 at 2bpp

// While the Hd66753 contains 5544 bytes of RAM (just enough to render 168 x 132
// as 2bpp), the RAM is _not_ linearly addressed! Table 4 on page 24 of the
// manual describes the layout:
//
// `xx` denotes invalid RAM addresses.
//
//        | 0x01 | 0x02 | .. | 0x14 | 0x15 | .... | 0x1f
// -------|------|------|----|------|------|------|------
// 0x0000 |      |      |    |      |  xx  |  xx  |  xx
// 0x0020 |      |      |    |      |  xx  |  xx  |  xx
// 0x0040 |      |      |    |      |  xx  |  xx  |  xx
//  ...   |      |      |    |      |  xx  |  xx  |  xx
// 0x1060 |      |      |    |      |  xx  |  xx  |  xx
//
// This unorthodox mapping results in a couple annoyances:
// - The Address Counter auto-update feature relies on some obtuse wrapping code
// - the Address Counter can't be used as a direct index into a
//   linearly-allocated emulated CGRAM array
//
// Point 2 can be mitigated by artificially allocating emulated CGRAM which
// includes the invalid RAM addresses. Yeah, it wastes some space, but
// it makes the code easier, so whatever ¯\_(ツ)_/¯
const EMU_CGRAM_WIDTH: usize = 256;
const EMU_CGRAM_BYTES: usize = (EMU_CGRAM_WIDTH * CGRAM_HEIGHT) * 2 / 8;
const EMU_CGRAM_LEN: usize = EMU_CGRAM_BYTES / 2; // addressed as 16-bit words

#[allow(clippy::unreadable_literal)]
const PALETTE: [u32; 4] = [0x000000, 0x686868, 0xb8b8b9, 0xffffff];

#[derive(Debug)]
struct Hd66753Renderer {
    kill_tx: chan::Sender<()>,
}

impl Hd66753Renderer {
    fn new(
        width: usize,
        height: usize,
        cgram: Arc<RwLock<[u16; EMU_CGRAM_LEN]>>,
        invert: Arc<AtomicBool>,
    ) -> Hd66753Renderer {
        let width = width + 8; // HACK

        let (kill_tx, kill_rx) = chan::bounded(1);

        let thread = move || {
            let mut buffer: Vec<u32> = vec![0; width * height];

            let mut window = Window::new(
                "iPod 4g",
                width,
                height,
                WindowOptions {
                    scale: minifb::Scale::X4,
                    resize: true,
                    ..WindowOptions::default()
                },
            )
            .expect("could not create minifb window");

            // ~60 fps
            window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

            while window.is_open() && kill_rx.is_empty() && !window.is_key_down(Key::Escape) {
                let cgram = *cgram.read().unwrap(); // avoid holding a lock
                let invert = invert.load(Ordering::Relaxed);

                // Only translate the chunk of CGRAM corresponding to visible pixels
                // (as set by the connected display's width / height)

                let cgram_window = cgram
                    .chunks_exact(EMU_CGRAM_WIDTH * 2 / 8 / 2)
                    .take(height)
                    .flat_map(|row| row.iter().take(width * 2 / 8 / 2).rev());

                let new_buf = cgram_window.flat_map(|w| {
                    // every 16 bits = 8 pixels
                    (0..8).rev().map(move |i| {
                        let idx = ((w >> (i * 2)) & 0b11) as usize;
                        if invert {
                            PALETTE[idx]
                        } else {
                            PALETTE[3 - idx]
                        }
                    })
                });

                // replace in-place
                buffer.splice(.., new_buf);

                assert_eq!(buffer.len(), width * height);

                window
                    .update_with_buffer(&buffer, width, height)
                    .expect("could not update minifb window");
            }

            // XXX: don't just std::process::exit when LCD window closes.
            std::process::exit(0)
        };

        let _handle = thread::Builder::new()
            .name("Hd66753 Renderer".into())
            .spawn(thread)
            .unwrap();

        Hd66753Renderer { kill_tx }
    }
}

impl Drop for Hd66753Renderer {
    fn drop(&mut self) {
        let _ = self.kill_tx.send(());
    }
}

#[derive(Default, Debug)]
struct InternalRegs {
    // Driver Output Control (R01)
    cms: bool,
    sgs: bool,
    nl: u8, // 5 bits
    // Contrast Control (R04)
    vr: u8, // 3 bits
    ct: u8, // 7 bits
    // Entry / Rotation mode (R05/R06)
    i_d: bool,
    am: u8, // 2 bits
    lg: u8, // 2 bits
    rt: u8, // 3 bits
    // Display Control (R07)
    spt: bool,
    gsh: u8, // 2 bits
    gsl: u8, // 2 bits
    rev: Arc<AtomicBool>,
    d: bool,
    // RAM Write Data Mask (R10)
    wm: u16,
}

/// Hitachi HD66753 168x132 monochrome LCD Controller
pub struct Hd66753 {
    renderer: Hd66753Renderer,

    // FIXME: not sure if there are separate latches for the command and data registers...
    write_byte_latch: Option<u8>,
    read_byte_latch: Option<u8>,

    /// Index Register
    ir: u16,
    /// Address counter
    ac: usize, // only 12 bits, indexes into cgram
    /// Graphics RAM
    cgram: Arc<RwLock<[u16; EMU_CGRAM_LEN]>>,

    ireg: InternalRegs,
}

impl std::fmt::Debug for Hd66753 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct("Hd66753")
            .field("renderer", &self.renderer)
            .field("write_byte_latch", &self.write_byte_latch)
            .field("read_byte_latch", &self.read_byte_latch)
            .field("ir", &self.ir)
            .field("ac", &self.ac)
            .field("cgram", &"[...]")
            .field("ireg", &self.ireg)
            .finish()
    }
}

impl Hd66753 {
    pub fn new_hle(width: usize, height: usize) -> Hd66753 {
        let cgram = Arc::new(RwLock::new([0; EMU_CGRAM_LEN]));
        let rev = Arc::new(AtomicBool::new(false));

        Hd66753 {
            renderer: Hd66753Renderer::new(width, height, Arc::clone(&cgram), Arc::clone(&rev)),
            ir: 0,
            ac: 0,
            cgram,
            write_byte_latch: None,
            read_byte_latch: None,
            ireg: InternalRegs {
                rev,
                ..InternalRegs::default()
            },
        }
    }

    fn handle_data(&mut self, val: u16) -> MemResult<()> {
        macro_rules! unimplemented_cmd {
            () => {
                return Err(FatalError(format!(
                    "unimplemented: LCD command {:#x?}",
                    self.ir
                )));
            };
        }

        match self.ir {
            // Driver output control
            0x01 => {
                self.ireg.cms = val.get_bit(9);
                self.ireg.sgs = val.get_bit(8);
                self.ireg.nl = val.get_bits(0..=4) as u8;
                // TODO?: use Driver Output control bits to control window size
            }
            0x02 => unimplemented_cmd!(),
            0x03 => unimplemented_cmd!(),
            // Contrast Control
            0x04 => {
                self.ireg.vr = val.get_bits(8..=10) as u8;
                self.ireg.ct = val.get_bits(0..=6) as u8;
                // TODO?: use Contrast Control bits to control rendered contrast
            }
            // Entry Mode
            0x05 => {
                self.ireg.i_d = val.get_bit(4);
                self.ireg.am = val.get_bits(2..=3) as u8;
                self.ireg.lg = val.get_bits(0..=1) as u8;

                if self.ireg.am == 0b11 {
                    return Err(ContractViolation {
                        msg: "0b11 is an invalid LCD EntryMode:AM value".into(),
                        severity: Error,
                        stub_val: None,
                    });
                }
            }
            // Rotation
            0x06 => self.ireg.rt = val.get_bits(0..=2) as u8,
            // Display Control
            0x07 => {
                self.ireg.spt = val.get_bit(8);
                self.ireg.gsh = val.get_bits(4..=5) as u8;
                self.ireg.gsl = val.get_bits(2..=3) as u8;
                self.ireg.rev.store(val.get_bit(1), Ordering::Relaxed);
                self.ireg.d = val.get_bit(0);
                // TODO: expose more LCD config data to renderer
            }
            // Cursor Control
            0x08 => unimplemented_cmd!(),
            // NOOP
            0x09 => {}
            // NOOP
            0x0a => {}
            // Horizontal Cursor Position
            0x0b => unimplemented_cmd!(),
            // Vertical Cursor Position
            0x0c => unimplemented_cmd!(),
            // 1st Screen Driving Position
            0x0d => unimplemented_cmd!(),
            // 1st Screen Driving Position
            0x0e => unimplemented_cmd!(),
            // NOTE: 0x0f isn't listed as a valid command.
            // 0x0f => {},
            // RAM Write Data Mask
            0x10 => self.ireg.wm = val,
            // RAM Address Set
            0x11 => self.ac = val as usize % 0x1080,
            // Write Data to CGRAM
            0x12 => {
                // Reference the Graphics Operation Function section of the manual for a
                // breakdown of what's happening here.

                let mut cgram = self.cgram.write().unwrap();

                // apply rotation
                let val = val.rotate_left(self.ireg.rt as u32 * 2);

                // apply the logical op
                let old_val = cgram[self.ac];
                let val = match self.ireg.lg {
                    0b00 => val, // replace
                    0b01 => old_val | val,
                    0b10 => old_val & val,
                    0b11 => old_val ^ val,
                    _ => unreachable!(),
                };

                // apply the write mask
                let val = (old_val & self.ireg.wm) | (val & !self.ireg.wm);

                // do the write
                cgram[self.ac] = val;

                // increment the ac appropriately
                let dx_ac = match self.ireg.am {
                    0b00 => 1,
                    0b01 => return Err(FatalError("unimplemented: vertical CGRAM write".into())),
                    0b10 => {
                        return Err(FatalError(
                            "unimplemented: two-word vertical CGRAM write".into(),
                        ))
                    }
                    0b11 => return Err(FatalError("EntryMode:AM cannot be set to 0b11".into())),
                    _ => unreachable!(),
                };

                self.ac = match self.ireg.i_d {
                    true => self.ac.wrapping_add(dx_ac),
                    false => self.ac.wrapping_sub(dx_ac),
                };

                self.ac %= 0x1080;

                // ... and handle wrapping behavior
                if self.ac & 0x1f > 0x14 {
                    self.ac = match self.ireg.i_d {
                        true => (self.ac & !0x1f) + 0x20,
                        false => (self.ac & !0x1f) + 0x14,
                    };
                }

                self.ac %= 0x1080;
            }
            invalid_cmd => {
                return Err(FatalError(format!(
                    "attempted to execute invalid LCD command {:#x?}",
                    invalid_cmd
                )))
            }
        }

        Ok(())
    }
}

impl Device for Hd66753 {
    fn kind(&self) -> &'static str {
        "HD 66753"
    }

    fn probe(&self, offset: u32) -> Probe {
        let reg = match offset {
            0x0 => "LCD Control",
            0x8 => "LCD Command",
            0x10 => "LCD Data",
            _ => return Probe::Unmapped,
        };

        Probe::Register(reg)
    }
}

impl Memory for Hd66753 {
    fn r32(&mut self, offset: u32) -> MemResult<u32> {
        if let Some(val) = self.read_byte_latch.take() {
            return Ok(val as u32);
        }

        let val: u16 = match offset {
            0x0 => 0,                   // HACK: Emulated LCD is never busy
            0x8 => self.ireg.ct as u16, // XXX: not currently tracking driving raster-row position
            _ => return Err(Unexpected),
        };

        self.read_byte_latch = Some(val as u8); // latch lower 8 bits
        Ok((val >> 8) as u32) // returning the higher 8 bits first
    }

    fn w32(&mut self, offset: u32, val: u32) -> MemResult<()> {
        let val = val as u8; // the iPod uses the controller using it's 8-bit interface

        let val = match self.write_byte_latch.take() {
            None => {
                self.write_byte_latch = Some(val as u8);
                return Ok(());
            }
            Some(hi) => (hi as u16) << 8 | (val as u16),
        };

        match offset {
            0x8 => {
                self.ir = val;

                if self.ir > 0x12 {
                    return Err(ContractViolation {
                        msg: format!("set invalid LCD Command: {:#04x?}", val),
                        severity: Error,
                        stub_val: None,
                    });
                }
            }
            0x10 => self.handle_data(val)?,
            _ => return Err(Unexpected),
        }

        Ok(())
    }
}
