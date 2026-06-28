//! BLE example: advertise as "T3S3-Msg" and expose a Nordic UART Service (NUS)
//! so a central (phone, laptop, the bundled ble.py) can connect and write a
//! message. Every write to the RX characteristic is printed over the serial
//! monitor and echoed back as a notification on the TX characteristic.
//!
//! Flash with `cargo run --example ble` (requires the `esp` toolchain + espflash).
//!
//! The BLE stack here is async: esp-radio provides the controller, esp-rtos the
//! scheduler + embassy executor, and trouble-host the GATT host. See README.md
//! for the version-matching notes (esp-radio/trouble-host must agree on bt-hci).

#![no_std]
#![no_main]
// the #[characteristic] macro expands to a borrow clippy flags; not our code.
#![allow(clippy::needless_borrows_for_generic_args)]

use embassy_executor::Spawner;
use embassy_futures::join::join;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use esp_radio::ble::controller::BleConnector;
use trouble_host::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// advertised name; this is what ble.py / a phone sees in a scan.
const DEVICE_NAME: &str = "T3S3-Msg";

/// max bytes accepted in a single write / sent in a notification.
const MSG_CAP: usize = 64;

const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2; // signalling + att

// nordic uart service: a de-facto standard "serial over ble" layout that
// generic tools recognise. rx = central -> peripheral, tx = peripheral -> central.
#[gatt_server]
struct Server {
    nus: NusService,
}

#[gatt_service(uuid = "6e400001-b5a3-f393-e0a9-e50e24dcca9e")]
struct NusService {
    #[characteristic(uuid = "6e400002-b5a3-f393-e0a9-e50e24dcca9e", write, read)]
    rx: heapless::Vec<u8, MSG_CAP>,
    #[characteristic(uuid = "6e400003-b5a3-f393-e0a9-e50e24dcca9e", read, notify)]
    tx: heapless::Vec<u8, MSG_CAP>,
}

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // esp-radio needs a heap; 72 KiB matches the trouble-host esp32 examples.
    esp_alloc::heap_allocator!(size: 72 * 1024);

    // the scheduler drives the radio's internal tasks and the embassy executor.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    let connector = BleConnector::new(peripherals.BT, Default::default()).unwrap();
    let controller: ExternalController<_, 20> = ExternalController::new(connector);

    run(controller).await;
}

async fn run<C>(controller: C)
where
    C: Controller,
{
    // a fixed random address keeps the device recognisable across reboots.
    let address = Address::random([0x42, 0x3f, 0x1a, 0x05, 0xe4, 0xff]);
    println!("ble: our address = {:?}", address.addr.raw());

    let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources).set_random_address(address);
    let Host {
        mut peripheral,
        runner,
        ..
    } = stack.build();

    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: DEVICE_NAME,
        appearance: &appearance::power_device::GENERIC_POWER_DEVICE,
    }))
    .unwrap();

    println!("ble: advertising as \"{DEVICE_NAME}\", waiting for a central to connect");

    let _ = join(ble_task(runner), async {
        loop {
            match advertise(DEVICE_NAME, &mut peripheral, &server).await {
                Ok(conn) => {
                    gatt_events_task(&server, &conn).await;
                }
                Err(e) => panic!("ble: advertise error: {e:?}"),
            }
        }
    })
    .await;
}

/// must run for the whole lifetime of the stack; it pumps the controller.
async fn ble_task<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) {
    loop {
        if let Err(e) = runner.run().await {
            panic!("ble: runner error: {e:?}");
        }
    }
}

/// handle GATT traffic until the central disconnects.
async fn gatt_events_task<P: PacketPool>(server: &Server<'_>, conn: &GattConnection<'_, '_, P>) {
    let rx = &server.nus.rx;
    let tx = &server.nus.tx;
    let reason = loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => break reason,
            GattConnectionEvent::Gatt { event } => {
                if let GattEvent::Write(write) = &event
                    && write.handle() == rx.handle
                {
                    let data = write.data();
                    match core::str::from_utf8(data) {
                        Ok(text) => {
                            println!("ble: received message: {text:?} ({} bytes)", data.len())
                        }
                        Err(_) => {
                            println!("ble: received {} bytes (non-utf8): {data:?}", data.len())
                        }
                    }
                    // echo it back on the tx characteristic so the central sees an ack.
                    if let Ok(echo) = heapless::Vec::<u8, MSG_CAP>::from_slice(data) {
                        let _ = tx.notify(conn, &echo).await;
                    }
                }
                match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(e) => println!("ble: error accepting gatt event: {e:?}"),
                }
            }
            _ => {}
        }
    };
    println!("ble: disconnected: {reason:?}");
}

/// advertise (connectable) and wait for a central to connect.
async fn advertise<'values, 'server, C: Controller>(
    name: &'values str,
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut adv_data = [0u8; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut adv_data[..],
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..len],
                scan_data: &[],
            },
        )
        .await?;
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    println!("ble: central connected");
    Ok(conn)
}
