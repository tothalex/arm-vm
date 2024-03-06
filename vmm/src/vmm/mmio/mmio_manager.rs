use kvm_ioctls::{IoEventAddress, VmFd};
use linux_loader::loader::Cmdline;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use vm_allocator::{AddressAllocator, AllocPolicy, IdAllocator};
use vm_superio::rtc_pl031::{NoEvents, Rtc};

use crate::vmm::device::{
    bus::{Bus, BusDevice},
    DeviceType,
};

use super::mmio_transport::MmioTransport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MMIODeviceInfo {
    /// Mmio address at which the device is registered.
    pub addr: u64,
    /// Mmio addr range length.
    pub len: u64,
    /// Used Irq line(s) for the device.
    pub irqs: Vec<u32>,
}

#[derive(Debug)]
pub struct MMIODeviceManager {
    pub(crate) bus: Bus,
    pub(crate) irq_allocator: IdAllocator,
    pub(crate) address_allocator: AddressAllocator,
    pub(crate) id_to_dev_info: HashMap<(DeviceType, String), MMIODeviceInfo>,
}

impl MMIODeviceManager {
    pub fn new() -> MMIODeviceManager {
        let mmio_base = 1 << 30;
        let mmio_size = 0x8000_0000 - 1 << 30;
        let irq_start = 32;
        let irq_end = 128;

        let irq_allocator = IdAllocator::new(irq_start, irq_end).unwrap();
        let address_allocator = AddressAllocator::new(mmio_base, mmio_size).unwrap();
        let bus = Bus::new();
        let id_to_dev_info = HashMap::new();

        MMIODeviceManager {
            irq_allocator,
            address_allocator,
            bus,
            id_to_dev_info,
        }
    }

    fn register_mmio_device(
        &mut self,
        identifier: (DeviceType, String),
        device_info: MMIODeviceInfo,
        device: Arc<Mutex<BusDevice>>,
    ) {
        self.bus.insert(device, device_info.addr, device_info.len);
        self.id_to_dev_info.insert(identifier, device_info);
    }

    pub fn register_mmio_virtio(
        &mut self,
        vm: &VmFd,
        device_id: String,
        mmio_device: MmioTransport,
        device_info: &MMIODeviceInfo,
    ) {
        if device_info.irqs.len() != 1 {
            panic!("invalid irq config");
        }

        let identifier;
        {
            let locked_device = mmio_device.locked_device();
            identifier = (DeviceType::Virtio(locked_device.device_type()), device_id);

            for (i, queue_evt) in locked_device.queue_events().iter().enumerate() {
                let io_addr = IoEventAddress::Mmio(device_info.addr + 0x50);

                vm.register_ioevent(queue_evt, &io_addr, u32::try_from(i).unwrap())
                    .unwrap();
            }

            vm.register_irqfd(locked_device.interrupt_evt(), device_info.irqs[0])
                .unwrap();
        }

        self.register_mmio_device(
            identifier,
            device_info.clone(),
            Arc::new(Mutex::new(BusDevice::MmioTransport(mmio_device))),
        )
    }

    pub fn register_mmio_virtio_for_boot(
        &mut self,
        vm: &VmFd,
        device_id: String,
        mmio_device: MmioTransport,
        _cmdline: &mut Cmdline,
    ) -> MMIODeviceInfo {
        let device_info = self.allocate_mmio_resources(1);
        self.register_mmio_virtio(vm, device_id, mmio_device, &device_info);

        device_info
    }

    pub fn register_mmio_serial(
        &mut self,
        vm: &VmFd,
        serial: Arc<Mutex<BusDevice>>,
        device_info_opt: Option<MMIODeviceInfo>,
    ) {
        let device_info = if let Some(device_info) = device_info_opt {
            device_info
        } else {
            self.allocate_mmio_resources(1)
        };

        vm.register_irqfd(
            serial
                .lock()
                .expect("Poisoned lock")
                .serial_ref()
                .unwrap()
                .serial
                .interrupt_evt(),
            device_info.irqs[0],
        )
        .unwrap();

        let identifier = (DeviceType::Serial, DeviceType::Serial.to_string());
        self.register_mmio_device(identifier, device_info, serial)
    }

    pub fn add_mmio_serial_to_cmdline(
        &self,
        cmdline: &mut Cmdline,
    ) -> Result<(), linux_loader::cmdline::Error> {
        let device_info = self
            .id_to_dev_info
            .get(&(DeviceType::Serial, DeviceType::Serial.to_string()))
            .unwrap();
        cmdline.insert("earlycon", &format!("uart,mmio,0x{:08x}", device_info.addr))
    }

    pub fn register_mmio_rtc(
        &mut self,
        rtc: Rtc<NoEvents>,
        device_info_opt: Option<MMIODeviceInfo>,
    ) {
        let device_info = if let Some(device_info) = device_info_opt {
            device_info
        } else {
            self.allocate_mmio_resources(1)
        };

        let identifier = (DeviceType::Rtc, DeviceType::Rtc.to_string());

        self.register_mmio_device(
            identifier,
            device_info,
            Arc::new(Mutex::new(BusDevice::RTCDevice(rtc))),
        )
    }

    fn allocate_mmio_resources(&mut self, irq_count: u32) -> MMIODeviceInfo {
        let irqs = (0..irq_count)
            .map(|_| self.irq_allocator.allocate_id())
            .collect::<vm_allocator::Result<_>>()
            .unwrap();

        let mmio_len = 0x1000;

        let device_info = MMIODeviceInfo {
            addr: self
                .address_allocator
                .allocate(mmio_len, mmio_len, AllocPolicy::FirstMatch)
                .unwrap()
                .start(),
            len: mmio_len,
            irqs,
        };

        device_info
    }
}
