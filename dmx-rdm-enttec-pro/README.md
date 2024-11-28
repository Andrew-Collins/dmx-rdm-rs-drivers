# dmx-rdm-enttec-pro

Library for using an Enttec DMX Pro (OpenDMX devices won't work)
with the [dmx-rdm-rs](https://crates.io/crates/dmx-rdm) library.

It is important that the Enttec DMX Pro is flashed with the RDM firmware.
Refer to the [official api documentation](https://cdn.enttec.com/pdf/assets/70304/70304_DMX_USB_PRO_API.pdf)
for more information.

## Example
```rust
use libftd2xx::Ftdi;
use dmx_rdm_enttec_pro::create_dmx_controller_from_enttec_pro;

fn main() {
    let mut dmx_controller = create_dmx_controller_from_enttec_pro(Ftdi::with_index(0).unwrap()).unwrap();
}
```

## License
Licensed under either of Apache License, Version 2.0 or MIT license at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in dmx-rdm-ftdi by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
