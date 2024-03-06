use std::sync::{atomic::AtomicU32, Arc};

use event_manager::{EventOps, EventSet, Events, MutEventSubscriber};
use vmm_sys_util::eventfd::EventFd;

use super::{IrqTrigger, VirtioDevice};

#[derive(Debug)]
pub struct Net {
    pub queue_events: Vec<EventFd>,
    pub irq_trigger: IrqTrigger,
    pub activate_event: EventFd,
}

impl Net {
    pub fn new() -> Net {
        let net_que_size = [256; 2];
        let mut queue_events = Vec::new();

        for _size in net_que_size {
            queue_events.push(EventFd::new(libc::EFD_NONBLOCK).unwrap());
        }

        let irq_trigger = IrqTrigger::new().unwrap();

        let activate_event = EventFd::new(libc::EFD_NONBLOCK).unwrap();

        Net {
            queue_events,
            irq_trigger,
            activate_event,
        }
    }
}

impl VirtioDevice for Net {
    fn device_type(&self) -> u32 {
        1
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

impl MutEventSubscriber for Net {
    fn process(&mut self, event: Events, ops: &mut EventOps) {
        todo!();
    }

    fn init(&mut self, ops: &mut EventOps) {
        dbg!("net device init called");
        if let Err(err) = ops.add(Events::new(&self.activate_event, EventSet::IN)) {
            panic!("Failed to register activate event: {}", err);
        }
    }
}
