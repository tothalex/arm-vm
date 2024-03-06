_THIS DOCUMENT WILL BE EXTENDED LATER_

# Virtual Devices

## Event Manager

This abstraction for implementing event based systems it is using epoll API for handling I/O notifications.

When a device is subscribed the init() function is called, this is responsible for setting up the event handling.

This might be the glue which sticks together the custom code with the system events from the vm.

## MMIO(memory-mapped IO management)

It is used to manage virtualized hardware devices.

### Bus

Device container.

### i8042 device

The i8042 is a microcontroller which acts as a interface between the cpu and PS/2 devices(keyboard, mouse) - in here we emulate for shutting down the computer. We will skip the metrics from here since I don't see the importance for now.

_this might be removed later since we don't need for aarch64_

### rtc device

The rtc device is a real-time-clock that keeps time of the current time and date.

### boot timer

This is not needed now.

### serial console device

Serial Communication interface purpose is to provide a interface to communicate with a device.

### block device

Block device is used for managing files/directories.

### net device

Net device is used for managing network interfaces.
