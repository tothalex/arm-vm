pub use self::{
    trigger::EventFdTrigger,
    wrapper::{SerialEventsWrapper, SerialWrapper},
};

pub mod out;
mod trigger;
mod wrapper;

pub type SerialDevice<I> = SerialWrapper<EventFdTrigger, SerialEventsWrapper, I>;
