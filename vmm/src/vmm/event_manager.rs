pub use event_manager::{EventManager as BaseEventManager, MutEventSubscriber, SubscriberOps};
use std::sync::{Arc, Mutex};

pub type EventManager = BaseEventManager<Arc<Mutex<dyn MutEventSubscriber>>>;
