use std::sync::{atomic::AtomicU32, Arc};

use event_manager::{EventOps, EventSet, Events, MutEventSubscriber};
use vmm_sys_util::eventfd::EventFd;

use super::{IrqTrigger, VirtioDevice};

#[derive(Debug)]
pub struct Block {
    pub queue_events: [EventFd; 1],
    pub irq_trigger: IrqTrigger,
    pub activate_event: EventFd,
}

impl Block {
    pub fn new() -> Block {
        let irq_trigger = IrqTrigger::new().unwrap();
        let queue_events = [EventFd::new(libc::EFD_NONBLOCK).unwrap()];
        let activate_event = EventFd::new(libc::EFD_NONBLOCK).unwrap();

        Block {
            queue_events,
            irq_trigger,
            activate_event,
        }
    }
}

impl VirtioDevice for Block {
    fn device_type(&self) -> u32 {
        2
    }

    fn queue_events(&self) -> &[EventFd] {
        &self.queue_events
    }

    fn interrupt_evt(&self) -> &EventFd {
        &self.irq_trigger.irq_evt
    }

    fn interrupt_status(&self) -> Arc<AtomicU32> {
        self.irq_trigger.irq_status.clone()
    }
}

impl MutEventSubscriber for Block {
    fn process(&mut self, event: Events, ops: &mut EventOps) {
        todo!();
    }

    fn init(&mut self, ops: &mut EventOps) {
        dbg!("block device init called");
        if let Err(err) = ops.add(Events::new(&self.activate_event, EventSet::IN)) {
            panic!("Failed to register activate event: {}", err);
        }
    }
}
