use std::cmp::min;
use std::num::Wrapping;
use std::sync::atomic::{fence, Ordering};

use crate::vmm::device::descriptor::DescriptorChain;
use crate::vmm::memory::{Address, Bytes, GuestAddress, GuestMemory};

pub enum QueueError {
    DescIndexOutOfBounds(u16),
    UsedRing(vm_memory::GuestMemoryError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A virtio queue's parameters.
pub struct Queue {
    /// The maximal size in elements offered by the device
    pub(crate) max_size: u16,

    /// The queue size in elements the driver selected
    pub size: u16,

    /// Indicates if the queue is finished with configuration
    pub ready: bool,

    /// Guest physical address of the descriptor table
    pub desc_table: GuestAddress,

    /// Guest physical address of the available ring
    pub avail_ring: GuestAddress,

    /// Guest physical address of the used ring
    pub used_ring: GuestAddress,

    pub(crate) next_avail: Wrapping<u16>,
    pub(crate) next_used: Wrapping<u16>,

    /// VIRTIO_F_RING_EVENT_IDX negotiated (notification suppression enabled)
    pub(crate) uses_notif_suppression: bool,
    /// The number of added used buffers since last guest kick
    pub(crate) num_added: Wrapping<u16>,
}

impl Queue {
    /// Constructs an empty virtio queue with the given `max_size`.
    pub fn new(max_size: u16) -> Queue {
        Queue {
            max_size,
            size: 0,
            ready: false,
            desc_table: GuestAddress(0),
            avail_ring: GuestAddress(0),
            used_ring: GuestAddress(0),
            next_avail: Wrapping(0),
            next_used: Wrapping(0),
            uses_notif_suppression: false,
            num_added: Wrapping(0),
        }
    }

    /// Maximum size of the queue.
    pub fn get_max_size(&self) -> u16 {
        self.max_size
    }

    /// Return the actual size of the queue, as the driver may not set up a
    /// queue as big as the device allows.
    pub fn actual_size(&self) -> u16 {
        min(self.size, self.max_size)
    }

    /// Validates the queue's in-memory layout is correct.
    pub fn is_layout_valid<M: GuestMemory>(&self, mem: &M) -> bool {
        let queue_size = usize::from(self.actual_size());
        let desc_table = self.desc_table;
        let desc_table_size = 16 * queue_size;
        let avail_ring = self.avail_ring;
        let avail_ring_size = 6 + 2 * queue_size;
        let used_ring = self.used_ring;
        let used_ring_size = 6 + 8 * queue_size;

        if !self.ready {
            dbg!("attempt to use virtio queue that is not marked ready");
            false
        } else if self.size > self.max_size || self.size == 0 || (self.size & (self.size - 1)) != 0
        {
            dbg!("virtio queue with invalid size: {}", self.size);
            false
        } else if desc_table.raw_value() & 0xf != 0 {
            dbg!("virtio queue descriptor table breaks alignment constraints");
            false
        } else if avail_ring.raw_value() & 0x1 != 0 {
            dbg!("virtio queue available ring breaks alignment constraints");
            false
        } else if used_ring.raw_value() & 0x3 != 0 {
            dbg!("virtio queue used ring breaks alignment constraints");
            false
        // range check entire descriptor table to be assigned valid guest physical addresses
        } else if mem.get_slice(desc_table, desc_table_size).is_err() {
            dbg!(
                "virtio queue descriptor table goes out of bounds: start:0x{:08x} size:0x{:08x}",
                desc_table.raw_value(),
                desc_table_size
            );
            false
        } else if mem.get_slice(avail_ring, avail_ring_size).is_err() {
            dbg!(
                "virtio queue available ring goes out of bounds: start:0x{:08x} size:0x{:08x}",
                avail_ring.raw_value(),
                avail_ring_size
            );
            false
        } else if mem.get_slice(used_ring, used_ring_size).is_err() {
            dbg!(
                "virtio queue used ring goes out of bounds: start:0x{:08x} size:0x{:08x}",
                used_ring.raw_value(),
                used_ring_size
            );
            false
        } else {
            true
        }
    }

    /// Validates that the queue's representation is correct.
    pub fn is_valid<M: GuestMemory>(&self, mem: &M) -> bool {
        if !self.is_layout_valid(mem) {
            false
        } else if self.len(mem) > self.max_size {
            dbg!(
                "virtio queue number of available descriptors {} is greater than queue max size {}",
                self.len(mem),
                self.max_size
            );
            false
        } else {
            true
        }
    }

    /// Returns the number of yet-to-be-popped descriptor chains in the avail ring.
    fn len<M: GuestMemory>(&self, mem: &M) -> u16 {
        debug_assert!(self.is_layout_valid(mem));

        (self.avail_idx(mem) - self.next_avail).0
    }

    /// Checks if the driver has made any descriptor chains available in the avail ring.
    pub fn is_empty<M: GuestMemory>(&self, mem: &M) -> bool {
        self.len(mem) == 0
    }

    /// Pop the first available descriptor chain from the avail ring.
    pub fn pop<'b, M: GuestMemory>(&mut self, mem: &'b M) -> Option<DescriptorChain<'b, M>> {
        debug_assert!(self.is_layout_valid(mem));

        let len = self.len(mem);
        // The number of descriptor chain heads to process should always
        // be smaller or equal to the queue size, as the driver should
        // never ask the VMM to process a available ring entry more than
        // once. Checking and reporting such incorrect driver behavior
        // can prevent potential hanging and Denial-of-Service from
        // happening on the VMM side.
        if len > self.actual_size() {
            // We are choosing to interrupt execution since this could be a potential malicious
            // driver scenario. This way we also eliminate the risk of repeatedly
            // logging and potentially clogging the microVM through the log system.
            panic!("The number of available virtio descriptors is greater than queue size!");
        }

        if len == 0 {
            return None;
        }

        self.do_pop_unchecked(mem)
    }

    /// Try to pop the first available descriptor chain from the avail ring.
    /// If no descriptor is available, enable notifications.
    pub fn pop_or_enable_notification<'b, M: GuestMemory>(
        &mut self,
        mem: &'b M,
    ) -> Option<DescriptorChain<'b, M>> {
        if !self.uses_notif_suppression {
            return self.pop(mem);
        }

        if self.try_enable_notification(mem) {
            return None;
        }

        self.do_pop_unchecked(mem)
    }

    /// Pop the first available descriptor chain from the avail ring.
    ///
    /// # Important
    /// This is an internal method that ASSUMES THAT THERE ARE AVAILABLE DESCRIPTORS. Otherwise it
    /// will retrieve a descriptor that contains garbage data (obsolete/empty).
    fn do_pop_unchecked<'b, M: GuestMemory>(
        &mut self,
        mem: &'b M,
    ) -> Option<DescriptorChain<'b, M>> {
        // This fence ensures all subsequent reads see the updated driver writes.
        fence(Ordering::Acquire);

        // We'll need to find the first available descriptor, that we haven't yet popped.
        // In a naive notation, that would be:
        // `descriptor_table[avail_ring[next_avail]]`.
        //
        // First, we compute the byte-offset (into `self.avail_ring`) of the index of the next
        // available descriptor. `self.avail_ring` stores the address of a `struct
        // virtq_avail`, as defined by the VirtIO spec:
        //
        // ```C
        // struct virtq_avail {
        //   le16 flags;
        //   le16 idx;
        //   le16 ring[QUEUE_SIZE];
        //   le16 used_event
        // }
        // ```
        //
        // We use `self.next_avail` to store the position, in `ring`, of the next available
        // descriptor index, with a twist: we always only increment `self.next_avail`, so the
        // actual position will be `self.next_avail % self.actual_size()`.
        // We are now looking for the offset of `ring[self.next_avail % self.actual_size()]`.
        // `ring` starts after `flags` and `idx` (4 bytes into `struct virtq_avail`), and holds
        // 2-byte items, so the offset will be:
        let index_offset = 4 + 2 * (self.next_avail.0 % self.actual_size());

        // `self.is_valid()` already performed all the bound checks on the descriptor table
        // and virtq rings, so it's safe to unwrap guest memory reads and to use unchecked
        // offsets.
        let desc_index: u16 = mem
            .read_obj(self.avail_ring.unchecked_add(u64::from(index_offset)))
            .unwrap();

        DescriptorChain::checked_new(mem, self.desc_table, self.actual_size(), desc_index).map(
            |dc| {
                self.next_avail += Wrapping(1);
                dc
            },
        )
    }

    /// Undo the effects of the last `self.pop()` call.
    /// The caller can use this, if it was unable to consume the last popped descriptor chain.
    pub fn undo_pop(&mut self) {
        self.next_avail -= Wrapping(1);
    }

    /// Puts an available descriptor head into the used ring for use by the guest.
    pub fn add_used<M: GuestMemory>(
        &mut self,
        mem: &M,
        desc_index: u16,
        len: u32,
    ) -> Result<(), QueueError> {
        debug_assert!(self.is_layout_valid(mem));

        if desc_index >= self.actual_size() {
            dbg!(
                "attempted to add out of bounds descriptor to used ring: {}",
                desc_index
            );
            return Err(QueueError::DescIndexOutOfBounds(desc_index));
        }

        let used_ring = self.used_ring;
        let next_used = u64::from(self.next_used.0 % self.actual_size());
        let used_elem = used_ring.unchecked_add(4 + next_used * 8);

        mem.write_obj(u32::from(desc_index), used_elem).unwrap();

        let len_addr = used_elem.unchecked_add(4);
        mem.write_obj(len, len_addr).unwrap();

        self.num_added += Wrapping(1);
        self.next_used += Wrapping(1);

        // This fence ensures all descriptor writes are visible before the index update is.
        fence(Ordering::Release);

        let next_used_addr = used_ring.unchecked_add(2);
        mem.write_obj(self.next_used.0, next_used_addr)
            .map_err(QueueError::UsedRing)
    }

    /// Fetch the available ring index (`virtq_avail->idx`) from guest memory.
    /// This is written by the driver, to indicate the next slot that will be filled in the avail
    /// ring.
    pub fn avail_idx<M: GuestMemory>(&self, mem: &M) -> Wrapping<u16> {
        // Bound checks for queue inner data have already been performed, at device activation time,
        // via `self.is_valid()`, so it's safe to unwrap and use unchecked offsets here.
        // Note: the `MmioTransport` code ensures that queue addresses cannot be changed by the
        // guest       after device activation, so we can be certain that no change has
        // occurred since the last `self.is_valid()` check.
        let addr = self.avail_ring.unchecked_add(2);
        Wrapping(mem.read_obj::<u16>(addr).unwrap())
    }

    /// Get the value of the used event field of the avail ring.
    #[inline(always)]
    pub fn used_event<M: GuestMemory>(&self, mem: &M) -> Wrapping<u16> {
        debug_assert!(self.is_layout_valid(mem));

        // We need to find the `used_event` field from the avail ring.
        let used_event_addr = self
            .avail_ring
            .unchecked_add(u64::from(4 + 2 * self.actual_size()));

        Wrapping(mem.read_obj::<u16>(used_event_addr).unwrap())
    }

    /// Helper method that writes `val` to the `avail_event` field of the used ring.
    fn set_avail_event<M: GuestMemory>(&mut self, val: u16, mem: &M) {
        debug_assert!(self.is_layout_valid(mem));

        let avail_event_addr = self
            .used_ring
            .unchecked_add(u64::from(4 + 8 * self.actual_size()));

        mem.write_obj(val, avail_event_addr).unwrap();
    }

    /// Try to enable notification events from the guest driver. Returns true if notifications were
    /// successfully enabled. Otherwise it means that one or more descriptors can still be consumed
    /// from the available ring and we can't guarantee that there will be a notification. In this
    /// case the caller might want to consume the mentioned descriptors and call this method again.
    pub fn try_enable_notification<M: GuestMemory>(&mut self, mem: &M) -> bool {
        debug_assert!(self.is_layout_valid(mem));

        // If the device doesn't use notification suppression, we'll continue to get notifications
        // no matter what.
        if !self.uses_notif_suppression {
            return true;
        }

        let len = self.len(mem);
        if len != 0 {
            // The number of descriptor chain heads to process should always
            // be smaller or equal to the queue size.
            if len > self.actual_size() {
                // We are choosing to interrupt execution since this could be a potential malicious
                // driver scenario. This way we also eliminate the risk of
                // repeatedly logging and potentially clogging the microVM through
                // the log system.
                panic!("The number of available virtio descriptors is greater than queue size!");
            }
            return false;
        }

        // Set the next expected avail_idx as avail_event.
        self.set_avail_event(self.next_avail.0, mem);

        // Make sure all subsequent reads are performed after `set_avail_event`.
        fence(Ordering::SeqCst);

        // If the actual avail_idx is different than next_avail one or more descriptors can still
        // be consumed from the available ring.
        self.next_avail.0 == self.avail_idx(mem).0
    }

    /// Enable notification suppression.
    pub fn enable_notif_suppression(&mut self) {
        self.uses_notif_suppression = true;
    }

    /// Check if we need to kick the guest.
    ///
    /// Please note this method has side effects: once it returns `true`, it considers the
    /// driver will actually be notified, and won't return `true` again until the driver
    /// updates `used_event` and/or the notification conditions hold once more.
    ///
    /// This is similar to the `vring_need_event()` method implemented by the Linux kernel.
    pub fn prepare_kick<M: GuestMemory>(&mut self, mem: &M) -> bool {
        debug_assert!(self.is_layout_valid(mem));

        // If the device doesn't use notification suppression, always return true
        if !self.uses_notif_suppression {
            return true;
        }

        // We need to expose used array entries before checking the used_event.
        fence(Ordering::SeqCst);

        let new = self.next_used;
        let old = self.next_used - self.num_added;
        let used_event = self.used_event(mem);

        self.num_added = Wrapping(0);

        new - used_event - Wrapping(1) < new - old
    }
}
