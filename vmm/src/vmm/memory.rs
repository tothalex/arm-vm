use std::fs::File;

use memfd::{FileSeal, Memfd, MemfdOptions, SealsHashSet};
pub use vm_memory::{
    bitmap::AtomicBitmap,
    mmap::{MmapRegionBuilder, MmapRegionError, NewBitmap},
    Address, ByteValued, Bytes, Error as VmMemoryError, FileOffset, GuestAddress, GuestMemory,
    GuestMemoryRegion,
};

pub enum MemoryError {
    FileError(std::io::Error),
    MmapRegionError(MmapRegionError),
    VmMemoryError(VmMemoryError),
}

pub type GuestMemoryMmap = vm_memory::GuestMemoryMmap<Option<AtomicBitmap>>;
pub type GuestRegionMmap = vm_memory::GuestRegionMmap<Option<AtomicBitmap>>;
pub type GuestMmapRegion = vm_memory::MmapRegion<Option<AtomicBitmap>>;

pub trait GuestMemoryExtension
where
    Self: Sized,
{
    fn with_file(file: &File, track_dirty_pages: bool) -> Result<Self, MemoryError>;

    fn from_raw_regions_file(
        regions: Vec<(FileOffset, GuestAddress, usize)>,
        track_dirty_pages: bool,
        shared: bool,
    ) -> Result<Self, MemoryError>;
}

impl GuestMemoryExtension for GuestMemoryMmap {
    fn with_file(file: &File, track_dirty_pages: bool) -> Result<Self, MemoryError> {
        let metadata = file.metadata().map_err(MemoryError::FileError)?;
        let mem_size = metadata.len() as usize;

        let mut offset: u64 = 0;
        let regions = arch_memory_regions(mem_size)
            .iter()
            .map(|(guest_address, region_size)| {
                let file_clone = file.try_clone().map_err(MemoryError::FileError)?;
                let file_offset = FileOffset::new(file_clone, offset);
                offset += *region_size as u64;
                Ok((file_offset, *guest_address, *region_size))
            })
            .collect::<Result<Vec<_>, MemoryError>>()?;

        Self::from_raw_regions_file(regions, track_dirty_pages, true)
    }

    fn from_raw_regions_file(
        regions: Vec<(FileOffset, GuestAddress, usize)>,
        track_dirty_pages: bool,
        shared: bool,
    ) -> Result<Self, MemoryError> {
        let prot = libc::PROT_READ | libc::PROT_WRITE;
        let flags = if shared {
            libc::MAP_NORESERVE | libc::MAP_SHARED
        } else {
            libc::MAP_NORESERVE | libc::MAP_PRIVATE
        };
        let regions = regions
            .into_iter()
            .map(|(file_offset, guest_address, region_size)| {
                let bitmap = match track_dirty_pages {
                    true => Some(AtomicBitmap::with_len(region_size)),
                    false => None,
                };
                let region = MmapRegionBuilder::new_with_bitmap(region_size, bitmap)
                    .with_mmap_prot(prot)
                    .with_mmap_flags(flags)
                    .with_file_offset(file_offset)
                    .build()
                    .map_err(MemoryError::MmapRegionError)?;
                GuestRegionMmap::new(region, guest_address).map_err(MemoryError::VmMemoryError)
            })
            .collect::<Result<Vec<_>, MemoryError>>()?;

        GuestMemoryMmap::from_regions(regions).map_err(MemoryError::VmMemoryError)
    }
}

pub fn create_memfd(size: usize) -> Memfd {
    let mem_size = size << 20;
    let opts = MemfdOptions::default().allow_sealing(true);
    let mem_file = match opts.create("guest_mem") {
        Ok(value) => value,
        Err(error) => panic!("{}", error),
    };

    let _ = match mem_file.as_file().set_len(mem_size as u64) {
        Ok(value) => value,
        Err(error) => panic!("{}", error),
    };

    let mut seals = SealsHashSet::new();
    seals.insert(FileSeal::SealShrink);
    seals.insert(FileSeal::SealGrow);
    match mem_file.add_seals(&seals) {
        Ok(value) => value,
        Err(error) => panic!("{}", error),
    };

    match mem_file.add_seal(FileSeal::SealSeal) {
        Ok(value) => value,
        Err(error) => panic!("{}", error),
    };

    mem_file
}

pub fn arch_memory_regions(size: usize) -> Vec<(GuestAddress, usize)> {
    vec![(GuestAddress(0x8000_0000), size)]
}

// Auxiliary function to get the address where the device tree blob is loaded.
pub fn get_fdt_addr(mem: &GuestMemoryMmap) -> u64 {
    // If the memory allocated is smaller than the size allocated for the FDT,
    // we return the start of the DRAM so that
    // we allow the code to try and load the FDT.

    if let Some(addr) = mem.last_addr().checked_sub(0x20_0000 as u64 - 1) {
        if mem.address_in_range(addr) {
            return addr.raw_value();
        }
    }

    0x8000_0000
}
