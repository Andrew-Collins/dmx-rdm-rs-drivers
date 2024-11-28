# dmx-rdm-ftdi

Library for using ftdi uarts, connected to rs485 transceivers with the [dmx-rdm-rs](https://crates.io/crates/dmx-rdm)
library. Most usb rs485 cables using a ftdi chipset will work. There are even once
that already have the XLR-connector attached to them.

## Limitations
- this is not for usage with Enttec DMX Pro devices; we will have to develop an additional driver for that.
- Enttec Open DMX devices will only work for DMX (not RDM) transmitting since they are hardwired to not allow data receiving.
- polling at the required rate is extremely cpu intensive

## License
Licensed under either of Apache License, Version 2.0 or MIT license at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in dmx-rdm-ftdi by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
