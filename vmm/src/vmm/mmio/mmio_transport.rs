use std::sync::{atomic::AtomicU32, Arc, Mutex, MutexGuard};

use crate::vmm::{device::VirtioDevice, memory::GuestMemoryMmap};

#[derive(Debug)]
pub struct MmioTransport {
    device: Arc<Mutex<dyn VirtioDevice>>,
    pub(crate) features_select: u32,
    pub(crate) acked_features_select: u32,
    pub(crate) queue_select: u32,
    pub(crate) device_status: u32,
    pub(crate) config_generation: u32,
    mem: GuestMemoryMmap,
    pub(crate) interrupt_status: Arc<AtomicU32>,
    pub is_vhost_user: bool,
}

impl MmioTransport {
    /// Constructs a new MMIO transport for the given virtio device.
    pub fn new(
        mem: GuestMemoryMmap,
        device: Arc<Mutex<dyn VirtioDevice>>,
        is_vhost_user: bool,
    ) -> MmioTransport {
        let interrupt_status = device.lock().expect("Poisoned lock").interrupt_status();

        MmioTransport {
            device,
            features_select: 0,
            acked_features_select: 0,
            queue_select: 0,
            device_status: 0,
            config_generation: 0,
            mem,
            interrupt_status,
            is_vhost_user,
        }
    }

    /// Gets the encapsulated locked VirtioDevice.
    pub fn locked_device(&self) -> MutexGuard<dyn VirtioDevice + 'static> {
        self.device.lock().expect("Poisoned lock")
    }
}
