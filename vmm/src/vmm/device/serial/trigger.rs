use std::fmt::Debug;
use std::io::{self};
use std::io::{Error, Result};
use std::ops::Deref;
use vm_superio::Trigger;

use vmm_sys_util::eventfd::EventFd;

#[derive(Debug)]
pub struct EventFdTrigger(EventFd);

impl Trigger for EventFdTrigger {
    type E = Error;

    fn trigger(&self) -> Result<()> {
        self.write(1)
    }
}

impl Deref for EventFdTrigger {
    type Target = EventFd;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl EventFdTrigger {
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(EventFdTrigger((**self).try_clone()?))
    }

    pub fn new(evt: EventFd) -> Self {
        Self(evt)
    }

    pub fn get_event(&self) -> EventFd {
        self.0.try_clone().unwrap()
    }
}
