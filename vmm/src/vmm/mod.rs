use kvm_bindings::kvm_userspace_memory_region;
use kvm_ioctls::{Kvm, VmFd};
use linux_loader;
use linux_loader::loader::{Cmdline, KernelLoader, KernelLoaderResult};
use std::fs::File;
use std::sync::{Arc, Mutex};
use vm_memory::{Address, Bytes, GuestAddress, GuestMemory, GuestMemoryRegion};
use vm_superio::{Rtc, Serial};
use vmm_sys_util::eventfd::EventFd;

use crate::vmm::device::DeviceType;
use crate::vmm::fdt::FdtBuilder;
use crate::vmm::memory::get_fdt_addr;

use self::cpu::Cpu;
use self::device::attach_virtio_device;
use self::device::block::Block;
use self::device::bus::BusDevice;
use self::device::net::Net;
use self::device::serial::out::SerialOut;
use self::device::serial::{EventFdTrigger, SerialEventsWrapper, SerialWrapper};
use self::event_manager::{EventManager, SubscriberOps};
use self::memory::{GuestMemoryExtension, GuestMemoryMmap};
use self::mmio::mmio_manager::MMIODeviceManager;

mod cpu;
mod device;
mod event_manager;
mod fdt;
mod gicv;
mod memory;
mod mmio;

pub const DEFAULT_KERNEL_CMDLINE: &str = "reboot=k panic=1 pci=off";

pub struct Vm {
    fd: VmFd,
    cpu: Cpu,
    memory: GuestMemoryMmap,
    memory_size: usize,
    mmio_device_manager: MMIODeviceManager,
    cmdline: Cmdline,
}

impl Vm {
    pub fn new(memory_size: usize) -> Vm {
        let guest_memory = Vm::create_memory(memory_size);

        let kernel = Vm::load_kernel(&guest_memory);

        let (kvm, kvm_fd) = Vm::create_kvm(&guest_memory);

        let cpu = Vm::create_cpu(&kvm_fd);

        let mut event_manager = EventManager::new().unwrap();

        let mut cmdline = Cmdline::try_from(DEFAULT_KERNEL_CMDLINE, 2048).unwrap();

        let mut mmio_device_manager = MMIODeviceManager::new();

        // attach block device
        let block = Arc::new(Mutex::new(Block::new()));
        attach_virtio_device(
            &guest_memory,
            &kvm_fd,
            &mut mmio_device_manager,
            &mut event_manager,
            "Root".to_string(),
            block,
            &mut cmdline,
            false,
        );

        // attach net device
        let net = Arc::new(Mutex::new(Net::new()));
        attach_virtio_device(
            &guest_memory,
            &kvm_fd,
            &mut mmio_device_manager,
            &mut event_manager,
            "Netif".to_string(),
            net,
            &mut cmdline,
            false,
        );

        // set stdout non-blocking
        Vm::set_stdout_nonblocking();

        // add serial device
        let serial_device = Vm::create_serial_device();
        event_manager.add_subscriber(serial_device.clone());
        mmio_device_manager.register_mmio_serial(&kvm_fd, serial_device, None);
        mmio_device_manager
            .add_mmio_serial_to_cmdline(&mut cmdline)
            .unwrap();

        // add rtc device
        let rtc_device = Rtc::new();
        mmio_device_manager.register_mmio_rtc(rtc_device, None);

        Vm {
            fd: kvm_fd,
            cpu,
            memory: guest_memory,
            mmio_device_manager,
            cmdline,
            memory_size,
        }
    }

    pub fn configure(&self) {
        self.cpu.init(&self.fd);
        self.cpu.configure_regs(&self.memory);

        let mut fdt = FdtBuilder::new();

        let rtc_info = self
            .mmio_device_manager
            .id_to_dev_info
            .get(&(DeviceType::Rtc, "Rtc".to_string()))
            .unwrap();
        fdt.with_rtc(rtc_info.addr, rtc_info.len);

        let serial_info = self
            .mmio_device_manager
            .id_to_dev_info
            .get(&(DeviceType::Serial, "Serial".to_string()))
            .unwrap();
        fdt.with_serial_console(serial_info.addr, serial_info.len);

        let block_info = self
            .mmio_device_manager
            .id_to_dev_info
            .get(&(DeviceType::Virtio(2), "Root".to_string()))
            .unwrap();
        fdt.add_virtio_device(block_info.addr, block_info.len, block_info.irqs[0]);

        let net_info = self
            .mmio_device_manager
            .id_to_dev_info
            .get(&(DeviceType::Virtio(1), "Netif".to_string()))
            .unwrap();
        fdt.add_virtio_device(net_info.addr, net_info.len, net_info.irqs[0]);

        fdt.with_cmdline(self.cmdline.as_cstring().unwrap().into_string().unwrap());
        fdt.with_mem_size(self.memory_size as u64);

        // write fdt to memory
        let raw = fdt.create_fdt().unwrap();
        let ftd_addr = GuestAddress(get_fdt_addr(&self.memory));
        self.memory
            .write_slice(raw.fdt_blob.as_slice(), ftd_addr)
            .unwrap();
    }

    fn create_memory(memory_size: usize) -> GuestMemoryMmap {
        let memfd = memory::create_memfd(memory_size);
        let guest_memory = match GuestMemoryMmap::with_file(memfd.as_file(), false) {
            Ok(value) => value,
            Err(_) => panic!("can't create guest memory"),
        };

        guest_memory
    }

    fn load_kernel(guest_memory: &GuestMemoryMmap) -> KernelLoaderResult {
        let mut kernel_image = match File::open("./kernel") {
            Ok(value) => value,
            Err(error) => panic!("{}", error),
        };
        let kernel = match linux_loader::loader::pe::PE::load(
            guest_memory,
            Some(GuestAddress(0x8000_0000)),
            &mut kernel_image,
            None,
        ) {
            Ok(value) => value,
            Err(error) => panic!("{}", error),
        };

        kernel
    }

    fn create_kvm(guest_memory: &GuestMemoryMmap) -> (Kvm, VmFd) {
        let kvm = match Kvm::new() {
            Ok(kvm) => kvm,
            Err(error) => panic!("{}", error),
        };
        let kvm_fd = match kvm.create_vm() {
            Ok(value) => value,
            Err(error) => panic!("{}", error),
        };

        // set kvm memory regions
        let _ = match guest_memory
            .iter()
            .enumerate()
            .try_for_each(|(index, region)| {
                let flags: u32 = 0u32;

                let memory_region = kvm_userspace_memory_region {
                    slot: u32::try_from(index).unwrap(),
                    guest_phys_addr: region.start_addr().raw_value(),
                    memory_size: region.len(),
                    userspace_addr: guest_memory.get_host_address(region.start_addr()).unwrap()
                        as u64,
                    flags,
                };

                unsafe { kvm_fd.set_user_memory_region(memory_region) }
            }) {
            Ok(value) => value,
            Err(error) => panic!("{}", error),
        };

        (kvm, kvm_fd)
    }

    fn create_cpu(kvm_fd: &VmFd) -> Cpu {
        let exit_evt = match EventFd::new(libc::EFD_NONBLOCK) {
            Ok(value) => value,
            Err(error) => panic!("{}", error),
        };

        let cpu = cpu::Cpu::new(0, kvm_fd, exit_evt);

        // setup interrupt handler
        let _ = match gicv::GICv2::create(kvm_fd, 1) {
            Ok(value) => value,
            Err(_) => panic!("cannot create gicv2"),
        };

        cpu
    }

    fn set_stdout_nonblocking() {
        // SAFETY: Call is safe since parameters are valid.
        let flags = unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_GETFL, 0) };
        if flags < 0 {
            panic!("Could not get Firecracker stdout flags.");
        }
        // SAFETY: Call is safe since parameters are valid.
        let rc =
            unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if rc < 0 {
            panic!("Could not set stdout to non-blocking.");
        }
    }

    fn create_serial_device() -> Arc<Mutex<BusDevice>> {
        let interrupt_evt = EventFdTrigger::new(EventFd::new(libc::EFD_NONBLOCK).unwrap());
        let kick_stdin_read_evt = EventFdTrigger::new(EventFd::new(libc::EFD_NONBLOCK).unwrap());

        let input = std::io::stdin();
        let out = std::io::stdout();

        let serial = Arc::new(Mutex::new(BusDevice::Serial(SerialWrapper {
            serial: Serial::with_events(
                interrupt_evt,
                SerialEventsWrapper {
                    buffer_ready_event_fd: Some(kick_stdin_read_evt),
                },
                SerialOut::Stdout(out),
            ),
            input: Some(input),
        })));

        serial
    }
}
