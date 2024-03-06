use vm_fdt::{Error, FdtWriter};

const PHANDLE_GIC: u32 = 1;

const AARCH64_FDT_MAX_SIZE: u64 = 0x200000;

// This indicates the start of DRAM inside the physical address space.
const AARCH64_PHYS_MEM_START: u64 = 0x80000000;

// This is the base address of MMIO devices.
const AARCH64_MMIO_BASE: u64 = 1 << 30;

const AARCH64_AXI_BASE: u64 = 0x40000000;

// These constants indicate the address space used by the ARM vGIC.
const AARCH64_GIC_DIST_SIZE: u64 = 0x10000;
const AARCH64_GIC_CPUI_SIZE: u64 = 0x20000;

// These constants indicate the placement of the GIC registers in the physical
// address space.
pub const AARCH64_GIC_DIST_BASE: u64 = AARCH64_AXI_BASE - AARCH64_GIC_DIST_SIZE;
pub const AARCH64_GIC_CPUI_BASE: u64 = AARCH64_GIC_DIST_BASE - AARCH64_GIC_CPUI_SIZE;
pub const AARCH64_GIC_REDIST_SIZE: u64 = 0x20000;

// These are specified by the Linux GIC bindings
const GIC_FDT_IRQ_NUM_CELLS: u32 = 3;
const GIC_FDT_IRQ_TYPE_SPI: u32 = 0;
const GIC_FDT_IRQ_TYPE_PPI: u32 = 1;
const GIC_FDT_IRQ_PPI_CPU_SHIFT: u32 = 8;
const GIC_FDT_IRQ_PPI_CPU_MASK: u32 = 0xff << GIC_FDT_IRQ_PPI_CPU_SHIFT;
const IRQ_TYPE_EDGE_RISING: u32 = 0x00000001;
const IRQ_TYPE_LEVEL_HIGH: u32 = 0x00000004;
const IRQ_TYPE_LEVEL_LOW: u32 = 0x00000008;
// PMU PPI interrupt, same as qemu
const AARCH64_PMU_IRQ: u32 = 7;

struct DeviceInfo {
    addr: u64,
    size: u64,
    irq: u32,
}

#[derive(Default)]
pub struct FdtBuilder {
    cmdline: String,
    mem_size: u64,
    virtio_devices: Vec<DeviceInfo>,
    serial_console: (u64, u64),
    rtc: (u64, u64),
}

pub struct Fdt {
    pub fdt_blob: Vec<u8>,
}

impl FdtBuilder {
    pub fn new() -> Self {
        FdtBuilder::default()
    }

    pub fn with_cmdline(&mut self, cmdline: String) -> &mut Self {
        self.cmdline = cmdline;
        self
    }

    pub fn with_mem_size(&mut self, mem_size: u64) -> &mut Self {
        self.mem_size = mem_size;
        self
    }

    pub fn add_virtio_device(&mut self, addr: u64, size: u64, irq: u32) -> &mut Self {
        self.virtio_devices.push(DeviceInfo { addr, size, irq });
        self
    }

    pub fn with_serial_console(&mut self, addr: u64, size: u64) -> &mut Self {
        self.serial_console = (addr, size);
        self
    }

    pub fn with_rtc(&mut self, addr: u64, size: u64) -> &mut Self {
        self.rtc = (addr, size);
        self
    }

    pub fn virtio_device_len(&self) -> usize {
        self.virtio_devices.len()
    }

    pub fn create_fdt(&self) -> Result<Fdt, Error> {
        let mut fdt = FdtWriter::new()?;

        let root_node = fdt.begin_node("")?;
        fdt.property_u32("interrupt-parent", 1)?;
        fdt.property_string("compatible", "linux,dummy-virt")?;
        fdt.property_u32("#address-cells", 0x2)?;
        fdt.property_u32("#size-cells", 0x2)?;

        // chosen node
        let chosen_node = fdt.begin_node("chosen")?;
        fdt.property_string("bootargs", self.cmdline.as_ref())?;
        fdt.end_node(chosen_node)?;

        // create memory node
        let mem_reg_prop = [0x80000000, self.mem_size];
        let memory_node = fdt.begin_node("memory")?;
        fdt.property_string("device_type", "memory")?;
        fdt.property_array_u64("reg", &mem_reg_prop)?;
        fdt.end_node(memory_node)?;

        // create cpu node
        let cpus_node = fdt.begin_node("cpus")?;
        fdt.property_u32("#address-cells", 0x1)?;
        fdt.property_u32("#size-cells", 0x0)?;
        let cpu_name = format!("cpu@{:x}", 0);
        let cpu_node = fdt.begin_node(&cpu_name)?;
        fdt.property_string("device_type", "cpu")?;
        fdt.property_string("compatible", "arm,arm-v8")?;
        fdt.property_string("enable-method", "psci")?;
        fdt.property_u32("reg", 0)?;
        fdt.end_node(cpu_node)?;
        fdt.end_node(cpus_node)?;

        // create gicv node
        let mut gic_reg_prop = [AARCH64_GIC_DIST_BASE, AARCH64_GIC_DIST_SIZE, 0, 0];
        let intc_node = fdt.begin_node("intc")?;
        fdt.property_string("compatible", "arm,gic-v3")?;
        gic_reg_prop[2] = AARCH64_GIC_DIST_BASE - (AARCH64_GIC_REDIST_SIZE);
        gic_reg_prop[3] = AARCH64_GIC_REDIST_SIZE;
        fdt.property_u32("#interrupt-cells", GIC_FDT_IRQ_NUM_CELLS)?;
        fdt.property_null("interrupt-controller")?;
        fdt.property_array_u64("reg", &gic_reg_prop)?;
        fdt.property_phandle(PHANDLE_GIC)?;
        fdt.property_u32("#address-cells", 2)?;
        fdt.property_u32("#size-cells", 2)?;
        fdt.end_node(intc_node)?;

        // create serial node
        let serial_node = fdt.begin_node(&format!("uart@{:x}", self.serial_console.0))?;
        fdt.property_string("compatible", "ns16550a")?;
        let serial_reg_prop = [self.serial_console.0, self.serial_console.1];
        fdt.property_array_u64("reg", &serial_reg_prop)?;
        const CLK_PHANDLE: u32 = 24;
        fdt.property_u32("clocks", CLK_PHANDLE)?;
        fdt.property_string("clock-names", "apb_pclk")?;
        let irq = [GIC_FDT_IRQ_TYPE_SPI, 4, IRQ_TYPE_EDGE_RISING];
        fdt.property_array_u32("interrupts", &irq)?;
        fdt.end_node(serial_node)?;

        // create rtc node
        let clock_node = fdt.begin_node("apb-pclk")?;
        fdt.property_u32("#clock-cells", 0)?;
        fdt.property_string("compatible", "fixed-clock")?;
        fdt.property_u32("clock-frequency", 24_000_000)?;
        fdt.property_string("clock-output-names", "clk24mhz")?;
        fdt.property_phandle(24)?;
        fdt.end_node(clock_node)?;
        let rtc_name = format!("rtc@{:x}", self.rtc.0);
        let reg = [self.rtc.0, self.rtc.1];
        let irq = [GIC_FDT_IRQ_TYPE_SPI, 33, IRQ_TYPE_LEVEL_HIGH];
        let rtc_node = fdt.begin_node(&rtc_name)?;
        fdt.property_string_list(
            "compatible",
            vec![String::from("arm,pl031"), String::from("arm,primecell")],
        )?;
        fdt.property_array_u64("reg", &reg)?;
        fdt.property_array_u32("interrupts", &irq)?;
        fdt.property_u32("clocks", CLK_PHANDLE)?;
        fdt.property_string("clock-names", "apb_pclk")?;
        fdt.end_node(rtc_node)?;

        // create timer node
        let irqs = [13, 14, 11, 10];
        let compatible = "arm,armv8-timer";
        let cpu_mask: u32 =
            (((1 << 1) - 1) << GIC_FDT_IRQ_PPI_CPU_SHIFT) & GIC_FDT_IRQ_PPI_CPU_MASK;
        let mut timer_reg_cells = Vec::new();
        for &irq in &irqs {
            timer_reg_cells.push(GIC_FDT_IRQ_TYPE_PPI);
            timer_reg_cells.push(irq);
            timer_reg_cells.push(cpu_mask | IRQ_TYPE_LEVEL_LOW);
        }
        let timer_node = fdt.begin_node("timer")?;
        fdt.property_string("compatible", compatible)?;
        fdt.property_array_u32("interrupts", &timer_reg_cells)?;
        fdt.property_null("always-on")?;
        fdt.end_node(timer_node)?;

        // create psci node
        let compatible = "arm,psci-0.2";
        let psci_node = fdt.begin_node("psci")?;
        fdt.property_string("compatible", compatible)?;
        fdt.property_string("method", "hvc")?;
        fdt.end_node(psci_node)?;

        // create pmu node
        let compatible = "arm,armv8-pmuv3";
        let cpu_mask: u32 =
            (((1 << 1) - 1) << GIC_FDT_IRQ_PPI_CPU_SHIFT) & GIC_FDT_IRQ_PPI_CPU_MASK;
        let irq = [
            GIC_FDT_IRQ_TYPE_PPI,
            AARCH64_PMU_IRQ,
            cpu_mask | IRQ_TYPE_LEVEL_HIGH,
        ];
        let pmu_node = fdt.begin_node("pmu")?;
        fdt.property_string("compatible", compatible)?;
        fdt.property_array_u32("interrupts", &irq)?;
        fdt.end_node(pmu_node)?;

        // create virtio device nodes
        for info in &self.virtio_devices {
            let virtio_mmio = fdt.begin_node(&format!("virtio_mmio@{:x}", info.addr))?;
            fdt.property_string("compatible", "virtio,mmio")?;
            fdt.property_array_u64("reg", &[info.addr, info.size])?;
            fdt.property_array_u32(
                "interrupts",
                &[GIC_FDT_IRQ_TYPE_SPI, info.irq, IRQ_TYPE_EDGE_RISING],
            )?;
            fdt.property_array_u32("interrupt-parent", &[PHANDLE_GIC])?;
            fdt.end_node(virtio_mmio)?;
        }

        fdt.end_node(root_node)?;

        Ok(Fdt {
            fdt_blob: fdt.finish()?,
        })
    }
}
