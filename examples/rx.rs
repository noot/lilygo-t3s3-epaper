//! Receive example: print every LoRa packet that arrives, with RSSI and SNR.
//!
//! Flash with `cargo run --example rx` (requires the `esp` toolchain + espflash).

#![no_std]
#![no_main]

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
    println!("sx1262 ready, listening at 915 MHz");

    let mut buf = [0u8; 255];
    loop {
        match radio.receive(&mut buf) {
            Ok(info) => {
                let payload = &buf[..info.len];
                println!(
                    "rx {} bytes  rssi={} dBm  snr={} dB  data={:?}",
                    info.len, info.rssi_dbm, info.snr_db, payload
                );
            }
            Err(e) => println!("rx error: {e:?}"),
        }
    }
}
