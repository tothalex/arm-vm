use std::num::Wrapping;
use vmm_sys_util::eventfd::EventFd;

const BUF_SIZE: usize = 16;

/// Offset of the status port (port 0x64)
const OFS_STATUS: u64 = 4;

/// Offset of the data port (port 0x60)
const OFS_DATA: u64 = 0;

/// i8042 commands
/// These values are written by the guest driver to port 0x64.
const CMD_READ_CTR: u8 = 0x20; // Read control register
const CMD_WRITE_CTR: u8 = 0x60; // Write control register
const CMD_READ_OUTP: u8 = 0xD0; // Read output port
const CMD_WRITE_OUTP: u8 = 0xD1; // Write output port
const CMD_RESET_CPU: u8 = 0xFE; // Reset CPU

/// i8042 status register bits
const SB_OUT_DATA_AVAIL: u8 = 0x0001; // Data available at port 0x60
const SB_I8042_CMD_DATA: u8 = 0x0008; // i8042 expecting command parameter at port 0x60
const SB_KBD_ENABLED: u8 = 0x0010; // 1 = kbd enabled, 0 = kbd locked

/// i8042 control register bits
const CB_KBD_INT: u8 = 0x0001; // kbd interrupt enabled
const CB_POST_OK: u8 = 0x0004; // POST ok (should always be 1)

/// Key scan codes
const KEY_CTRL: u16 = 0x0014;
const KEY_ALT: u16 = 0x0011;
const KEY_DEL: u16 = 0xE071;

#[derive(Debug)]
pub struct I8042Device {
    /// CPU reset eventfd. We will set this event when the guest issues CMD_RESET_CPU.
    reset_evt: EventFd,

    /// Keyboard interrupt event (IRQ 1).
    kbd_interrupt_evt: EventFd,

    /// The i8042 status register.
    status: u8,

    /// The i8042 control register.
    control: u8,

    /// The i8042 output port.
    outp: u8,

    /// The last command sent to port 0x64.
    cmd: u8,

    /// The internal i8042 data buffer.
    buf: [u8; BUF_SIZE],
    bhead: Wrapping<usize>,
    btail: Wrapping<usize>,
}

impl I8042Device {
    /// Constructs an i8042 device that will signal the given event when the guest requests it.
    pub fn new(reset_evt: EventFd, kbd_interrupt_evt: EventFd) -> I8042Device {
        I8042Device {
            reset_evt,
            kbd_interrupt_evt,
            control: CB_POST_OK | CB_KBD_INT,
            cmd: 0,
            outp: 0,
            status: SB_KBD_ENABLED,
            buf: [0; BUF_SIZE],
            bhead: Wrapping(0),
            btail: Wrapping(0),
        }
    }
}
