use event_manager::{EventOps, EventSet, Events, MutEventSubscriber};
use std::fmt::Debug;
use std::io::Read;
use std::os::fd::RawFd;
use std::os::unix::io::AsRawFd;
use vm_superio::serial::{NoEvents, SerialEvents};
use vm_superio::{Serial, Trigger};

use super::out::SerialOut;
use super::trigger::EventFdTrigger;

#[derive(Debug)]
pub struct SerialWrapper<T: Trigger, EV: SerialEvents, I: Read + AsRawFd + Send> {
    /// Serial device object.
    pub serial: Serial<T, EV, SerialOut>,
    /// Input to the serial device (needs to be readable).
    pub input: Option<I>,
}

fn is_fifo(fd: RawFd) -> bool {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();

    // SAFETY: No unsafety can be introduced by passing in an invalid file descriptor to fstat,
    // it will return -1 and set errno to EBADF. The pointer passed to fstat is valid for writing
    // a libc::stat structure.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } < 0 {
        return false;
    }

    // SAFETY: We can safely assume the libc::stat structure to be initialized, as libc::fstat
    // returning 0 guarantees that the memory is now initialized with the requested file metadata.
    let stat = unsafe { stat.assume_init() };

    (stat.st_mode & libc::S_IFIFO) != 0
}

impl<I: Read + AsRawFd + Send + Debug> MutEventSubscriber
    for SerialWrapper<EventFdTrigger, SerialEventsWrapper, I>
{
    fn process(&mut self, event: Events, ops: &mut EventOps) {
        todo!();
    }

    fn init(&mut self, ops: &mut EventOps) {
        dbg!("serial device init called");
        if self.input.is_some() && self.serial.events().buffer_ready_event_fd.is_some() {
            let serial_fd = self.input.as_ref().map_or(-1, |input| input.as_raw_fd());
            let buf_ready_evt = self
                .serial
                .events()
                .buffer_ready_event_fd
                .as_ref()
                .map_or(-1, |buf_ready| buf_ready.as_raw_fd());

            if unsafe { libc::isatty(serial_fd) } == 1 || is_fifo(serial_fd) {
                if let Err(err) = ops.add(Events::new(&serial_fd, EventSet::IN)) {
                    panic!("Failed to register serial input fd: {}", err);
                }
            }
            if let Err(err) = ops.add(Events::new(&buf_ready_evt, EventSet::IN)) {
                panic!("Failed to register serial buffer ready event: {}", err);
            }
        }
    }
}

#[derive(Debug)]
pub struct SerialEventsWrapper {
    pub buffer_ready_event_fd: Option<EventFdTrigger>,
}

impl SerialEvents for SerialEventsWrapper {
    fn buffer_read(&self) {}

    fn out_byte(&self) {}

    fn tx_lost_byte(&self) {}

    fn in_buffer_empty(&self) {
        match self
            .buffer_ready_event_fd
            .as_ref()
            .map_or(Ok(()), |buf_ready| buf_ready.write(1))
        {
            Ok(_) => (),
            Err(err) => panic!(
                "Could not signal that serial device buffer is ready: {:?}",
                err
            ),
        }
    }
}
