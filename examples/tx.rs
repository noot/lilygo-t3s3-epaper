//! Transmit example: send an incrementing LoRa packet every ~3 seconds.
//!
//! Flash with `cargo run --example tx` (requires the `esp` toolchain + espflash).

#![no_std]
#![no_main]

use embedded_hal::delay::DelayNs as _;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::main;
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_println::println;

use lilygo_t3s3_epaper::sx1262::{Config, Sx1262};

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // lora radio spi bus: sck=5, mosi=6, miso=3 (see board module).
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(8))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO5)
    .with_mosi(peripherals.GPIO6)
    .with_miso(peripherals.GPIO3);

    let nss = Output::new(peripherals.GPIO7, Level::High, OutputConfig::default());
    let spi_dev = ExclusiveDevice::new(spi, nss, Delay::new()).unwrap();

    let rst = Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());
    let busy = Input::new(
        peripherals.GPIO34,
        InputConfig::default().with_pull(Pull::None),
    );
    let dio1 = Input::new(
        peripherals.GPIO33,
        InputConfig::default().with_pull(Pull::None),
    );

    let mut radio = Sx1262::new(spi_dev, rst, busy, dio1, Delay::new(), Config::default());
    radio.init().unwrap();
    println!("sx1262 ready, transmitting at 915 MHz");

    let mut delay = Delay::new();
    let mut counter: u32 = 0;
    loop {
        let mut payload = *b"ping 00000000";
        write_hex(&mut payload[5..], counter);
        match radio.transmit(&payload) {
            Ok(()) => println!("tx #{counter}"),
            Err(e) => println!("tx error: {e:?}"),
        }
        counter = counter.wrapping_add(1);
        delay.delay_ms(3_000);
    }
}

/// write `value` as 8 lowercase hex digits into `out`.
fn write_hex(out: &mut [u8], value: u32) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in out.iter_mut().enumerate().take(8) {
        let shift = (7 - i) * 4;
        *byte = HEX[((value >> shift) & 0xF) as usize];
    }
}
