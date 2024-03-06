use std::any::Any;
use std::fmt::{self, Debug};
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};

use event_manager::{MutEventSubscriber, SubscriberOps};

use kvm_ioctls::VmFd;
use linux_loader::loader::Cmdline;
use std::io::{self};
use std::sync::{Arc, Mutex};
use vm_superio::Trigger;
use vmm_sys_util::eventfd::EventFd;

use crate::vmm::event_manager::EventManager;
use crate::vmm::memory::GuestMemoryMmap;
use crate::vmm::mmio::mmio_manager::MMIODeviceManager;
use crate::vmm::mmio::mmio_transport::MmioTransport;

mod descriptor;
mod i8042;
mod queue;

pub mod block;
pub mod bus;
pub mod net;
pub mod serial;

pub trait AsAny {
    /// Return the immutable any encapsulated object.
    fn as_any(&self) -> &dyn Any;

    /// Return the mutable encapsulated any object.
    fn as_mut_any(&mut self) -> &mut dyn Any;
}

impl<T: Any> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Debug)]
pub enum DeviceState {
    Inactive,
    Activated(GuestMemoryMmap),
}

impl DeviceState {
    /// Checks if the device is activated.
    pub fn is_activated(&self) -> bool {
        match self {
            DeviceState::Inactive => false,
            DeviceState::Activated(_) => true,
        }
    }

    /// Gets the memory attached to the device if it is activated.
    pub fn mem(&self) -> Option<&GuestMemoryMmap> {
        match self {
            DeviceState::Activated(ref mem) => Some(mem),
            DeviceState::Inactive => None,
        }
    }
}

#[derive(Debug)]
pub enum IrqType {
    /// Interrupt triggered by change in config.
    Config,
    /// Interrupt triggered by used vring buffers.
    Vring,
}

/// Helper struct that is responsible for triggering guest IRQs
#[derive(Debug)]
pub struct IrqTrigger {
    pub(crate) irq_status: Arc<AtomicU32>,
    pub(crate) irq_evt: EventFd,
}

impl IrqTrigger {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            irq_status: Arc::new(AtomicU32::new(0)),
            irq_evt: EventFd::new(libc::EFD_NONBLOCK)?,
        })
    }

    pub fn trigger_irq(&self, irq_type: IrqType) -> Result<(), std::io::Error> {
        let irq = match irq_type {
            IrqType::Config => 0x02,
            IrqType::Vring => 0x01,
        };
        self.irq_status.fetch_or(irq, Ordering::SeqCst);

        self.irq_evt
            .write(1)
            .map_err(|err| {
                panic!("Failed to send irq to the guest: {:?}", err);
            })
            .unwrap();

        Ok(())
    }
}

pub trait VirtioDevice: AsAny + Send {
    fn device_type(&self) -> u32;

    fn queue_events(&self) -> &[EventFd];

    fn interrupt_evt(&self) -> &EventFd;

    fn interrupt_status(&self) -> Arc<AtomicU32>;

    fn reset(&mut self) -> Option<(EventFd, Vec<EventFd>)> {
        None
    }
}

impl fmt::Debug for dyn VirtioDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VirtioDevice type {}", self.device_type())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Copy)]
pub enum DeviceType {
    Virtio(u32),
    Serial,
    Rtc,
}

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn attach_virtio_device<T: 'static + VirtioDevice + MutEventSubscriber + Debug>(
    guest_memory: &GuestMemoryMmap,
    vm_fd: &VmFd,
    mmio_device_manager: &mut MMIODeviceManager,
    event_manager: &mut EventManager,
    id: String,
    device: Arc<Mutex<T>>,
    cmdline: &mut Cmdline,
    is_vhost_user: bool,
) {
    event_manager.add_subscriber(device.clone());

    let device = MmioTransport::new(guest_memory.clone(), device, is_vhost_user);

    mmio_device_manager.register_mmio_virtio_for_boot(vm_fd, id, device, cmdline);
}
