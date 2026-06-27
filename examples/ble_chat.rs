//! BLE chat example: expose a Nordic UART Service (NUS) so a phone can connect
//! over bluetooth low energy and exchange text. Whatever the phone writes is
//! echoed back to it and shown on the e-paper display.
//!
//! this is step one of the phone<->lora bridge: it proves the ble stack brings
//! up and a phone can connect, with no radio wired in yet. pair from an android
//! app that speaks NUS (e.g. "Serial Bluetooth Terminal" in BLE mode).
//!
//! Flash with `cargo run --example ble_chat`.

#![no_std]
#![no_main]
// the gatt_service macro expands to borrows clippy flags as needless.
#![allow(clippy::needless_borrows_for_generic_args)]

use core::fmt::Write as _;

use embassy_futures::join::join3;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_10X20};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle};
use embedded_graphics::text::Text;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use esp_radio::ble::controller::BleConnector;
use heapless::Vec;
use trouble_host::prelude::*;

use lilygo_t3s3_epaper::ssd1680::{Display, Rotation};

esp_bootloader_esp_idf::esp_app_desc!();

/// max ble connections (one phone at a time).
const CONNECTIONS_MAX: usize = 1;
/// l2cap channels: signalling + att.
const L2CAP_CHANNELS_MAX: usize = 2;
/// longest text chunk we carry in either direction.
const MSG_CAP: usize = 64;
/// nus service uuid (6e400001-...) in little-endian byte order, for advertising.
const NUS_SERVICE_UUID_LE: [u8; 16] = [
    0x9e, 0xca, 0xdc, 0x24, 0x0e, 0xe5, 0xa9, 0xe0, 0x93, 0xf3, 0xa3, 0xb5, 0x01, 0x00, 0x40, 0x6e,
];
/// do a clean full refresh every this many updates to clear partial-refresh ghosting.
const FULL_REFRESH_EVERY: u32 = 10;

// nordic uart service: 6e400001 (service), 6e400002 rx (phone -> device),
// 6e400003 tx (device -> phone). the gatt macros require uuid string literals.
#[gatt_server]
struct Server {
    nus: NusService,
}

#[gatt_service(uuid = "6e400001-b5a3-f393-e0a9-e50e24dcca9e")]
struct NusService {
    #[characteristic(
        uuid = "6e400002-b5a3-f393-e0a9-e50e24dcca9e",
        write,
        write_without_response
    )]
    rx: Vec<u8, MSG_CAP>,
    #[characteristic(uuid = "6e400003-b5a3-f393-e0a9-e50e24dcca9e", notify)]
    tx: Vec<u8, MSG_CAP>,
}

/// latest text received from the phone, handed to the display task.
type DisplaySignal = Signal<CriticalSectionRawMutex, Vec<u8, MSG_CAP>>;

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) {
    // esp-radio's ble controller needs the full clock to actually get on air;
    // the tx/rx examples avoid max because of esp-hal's MISO input-delay bug on
    // the radio spi bus, but that bus isn't used here and the display spi is
    // write-only, so max is safe for this example.
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // the ble controller needs a heap; 72 KiB is what the trouble esp32 examples use.
    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_ints = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_ints.software_interrupt0);

    // e-paper display on spi3: sclk=14, mosi=11, cs=15.
    let disp_spi = Spi::new(
        peripherals.SPI3,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(4))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO14)
    .with_mosi(peripherals.GPIO11);
    let disp_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());
    let disp_dev = ExclusiveDevice::new(disp_spi, disp_cs, Delay::new()).unwrap();
    let disp_dc = Output::new(peripherals.GPIO16, Level::Low, OutputConfig::default());
    let disp_rst = Output::new(peripherals.GPIO47, Level::High, OutputConfig::default());
    let disp_busy = Input::new(
        peripherals.GPIO48,
        InputConfig::default().with_pull(Pull::None),
    );
    let mut display = Display::new(disp_dev, disp_dc, disp_rst, disp_busy, Delay::new());
    display.set_rotation(Rotation::Rotate270); // landscape, 250 x 122
    display.init().unwrap();
    render(
        &mut display,
        "BLE chat",
        "advertising...",
        "name: lora-bridge",
        "",
    );
    display.refresh().unwrap();

    let connector = BleConnector::new(peripherals.BT, Default::default()).unwrap();
    let controller: ExternalController<_, 20> = ExternalController::new(connector);

    let address = Address::random([0xff, 0x10, 0x05, 0x05, 0xe4, 0xff]);
    println!("ble address = {:?}", address);

    let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources).set_random_address(address);
    let Host {
        mut peripheral,
        runner,
        ..
    } = stack.build();

    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: "lora-bridge",
        appearance: &appearance::power_device::GENERIC_POWER_DEVICE,
    }))
    .unwrap();

    let signal: DisplaySignal = Signal::new();

    // render received text on the e-paper whenever a new message arrives. this
    // lives inline (not a helper) so it can call the concrete display's inherent
    // refresh methods; refreshes here briefly block the executor, which is fine
    // for an echo demo.
    let display_fut = async {
        let mut counter: u32 = 0;
        loop {
            let msg = signal.wait().await;
            let mut line = FmtBuf::new();
            match core::str::from_utf8(&msg) {
                Ok(text) => {
                    let _ = write!(line, "msg: {text}");
                }
                Err(_) => {
                    let _ = write!(line, "msg: <binary>");
                }
            }
            let mut count = FmtBuf::new();
            let _ = write!(count, "#{counter}");
            render(&mut display, "BLE chat", count.as_str(), line.as_str(), "");
            if counter.is_multiple_of(FULL_REFRESH_EVERY) {
                let _ = display.refresh();
            } else {
                let _ = display.refresh_partial();
            }
            counter = counter.wrapping_add(1);
        }
    };

    join3(
        ble_task(runner),
        connection_loop(&mut peripheral, &server, &signal),
        display_fut,
    )
    .await;
}

/// background task that drives the host stack; must run for the whole program.
async fn ble_task<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) {
    loop {
        if let Err(e) = runner.run().await {
            println!("ble runner error: {e:?}");
        }
    }
}

/// advertise, accept one connection, serve it until it drops, then repeat.
async fn connection_loop<'values, C: Controller>(
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &Server<'values>,
    signal: &DisplaySignal,
) {
    loop {
        println!("advertising as lora-bridge...");
        let conn = match advertise(peripheral, server).await {
            Ok(conn) => conn,
            Err(e) => {
                println!("advertise error: {e:?}");
                continue;
            }
        };
        println!("phone connected");
        serve(server, &conn, signal).await;
        println!("phone disconnected");
    }
}

/// build the advertising payload and wait for a central to connect.
async fn advertise<'values, 'server, C: Controller>(
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    // advertise the NUS service uuid so serial terminal apps recognise and list
    // the device; the 128-bit uuid plus the name overflows the 31-byte advert,
    // so the name goes in the scan response, which active scanners request.
    let mut adv_data = [0u8; 31];
    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::ServiceUuids128(&[NUS_SERVICE_UUID_LE]),
        ],
        &mut adv_data[..],
    )?;
    let mut scan_data = [0u8; 31];
    let scan_len = AdStructure::encode_slice(
        &[AdStructure::CompleteLocalName(b"lora-bridge")],
        &mut scan_data[..],
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..adv_len],
                scan_data: &scan_data[..scan_len],
            },
        )
        .await?;
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    Ok(conn)
}

/// handle one connection: echo every write back to the phone and to the display.
async fn serve<P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
    signal: &DisplaySignal,
) {
    let rx = &server.nus.rx;
    let tx = &server.nus.tx;
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                println!("disconnect reason: {reason:?}");
                break;
            }
            GattConnectionEvent::Gatt { event } => {
                println!("gatt event");
                if let GattEvent::Write(write) = &event
                    && write.handle() == rx.handle
                {
                    let data = write.data();
                    let msg: Vec<u8, MSG_CAP> =
                        Vec::from_slice(&data[..data.len().min(MSG_CAP)]).unwrap_or_default();
                    println!("rx from phone: {:?}", core::str::from_utf8(&msg));
                    // echo it straight back to the phone.
                    if tx.notify(conn, &msg).await.is_err() {
                        println!("notify failed");
                    }
                    signal.signal(msg);
                }
                match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(e) => println!("gatt reply error: {e:?}"),
                }
            }
            _ => println!("other conn event"),
        }
    }
}

/// draw a title, a rule and up to three body lines into the framebuffer.
fn render<D>(display: &mut D, title: &str, line1: &str, line2: &str, line3: &str)
where
    D: DrawTarget<Color = BinaryColor>,
{
    let title_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let body = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let rule = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

    let _ = display.clear(BinaryColor::Off);
    let _ = Text::new(title, Point::new(8, 24), title_style).draw(display);
    let _ = Line::new(Point::new(8, 32), Point::new(242, 32))
        .into_styled(rule)
        .draw(display);
    let _ = Text::new(line1, Point::new(8, 52), body).draw(display);
    let _ = Text::new(line2, Point::new(8, 68), body).draw(display);
    let _ = Text::new(line3, Point::new(8, 84), body).draw(display);
}

/// a tiny fixed-capacity buffer that implements `core::fmt::Write` so `write!`
/// can format strings without an allocator.
struct FmtBuf {
    buf: [u8; 48],
    len: usize,
}

impl FmtBuf {
    fn new() -> Self {
        Self {
            buf: [0; 48],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl core::fmt::Write for FmtBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let n = bytes.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        Ok(())
    }
}
