use kvm_bindings::kvm_vcpu_init;
use kvm_bindings::{PSR_MODE_EL1h, PSR_A_BIT, PSR_D_BIT, PSR_F_BIT, PSR_I_BIT};
use kvm_bindings::{KVM_REG_ARM64, KVM_REG_ARM_CORE, KVM_REG_SIZE_U64};
use kvm_ioctls::{VcpuFd, VmFd};
use vmm_sys_util::eventfd::EventFd;

use crate::vmm::memory::*;

pub const AARCH64_FDT_MAX_SIZE: u64 = 0x200000;

#[macro_use]
mod regs;

pub struct Cpu {
    pub index: u8,
    pub fd: VcpuFd,
    mpidr: u64,
    kvi: Option<kvm_vcpu_init>,

    exit_evt: EventFd,
}

impl Cpu {
    pub fn new(index: u8, kvm_fd: &VmFd, exit_evt: EventFd) -> Self {
        let kvm_cpu = match kvm_fd.create_vcpu(index.into()) {
            Ok(value) => value,
            Err(error) => panic!("{}", error),
        };

        Cpu {
            index,
            fd: kvm_cpu,
            mpidr: 0,
            kvi: None,

            exit_evt,
        }
    }

    pub fn init(&self, vm_fd: &VmFd) {
        let mut kvi: kvm_vcpu_init = kvm_vcpu_init::default();
        vm_fd.get_preferred_target(&mut kvi).unwrap();

        kvi.features[0] |= 1 << kvm_bindings::KVM_ARM_VCPU_PSCI_0_2;

        self.fd.vcpu_init(&kvi).unwrap();
    }

    pub fn configure_regs(&self, guest_memory: &GuestMemoryMmap) {
        let mut data: u64;
        let mut reg_id: u64;

        data = (PSR_D_BIT | PSR_A_BIT | PSR_I_BIT | PSR_F_BIT | PSR_MODE_EL1h).into();

        reg_id = arm64_core_reg!(pstate);

        self.fd.set_one_reg(reg_id, &data.to_le_bytes()).unwrap();

        let mut fdt_offset: u64 = guest_memory.iter().map(|region| region.len()).sum();
        fdt_offset = fdt_offset - AARCH64_FDT_MAX_SIZE - 0x10000;
        data = (0x80000000 + fdt_offset) as u64;
        reg_id = arm64_core_reg!(regs);

        self.fd.set_one_reg(reg_id, &data.to_le_bytes()).unwrap();
    }
}
