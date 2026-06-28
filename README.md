# lilygo-t3s3-epaper

Firmware support for the **LilyGO T3-S3** board (ESP32-S3 + SX1262 LoRa + SSD1680
e-paper). The library (`src/`) is pure `embedded-hal` 1.0 and platform-agnostic;
the device-specific glue lives in the examples.

## Examples

| Example   | What it does |
|-----------|--------------|
| `display` | Draws text on the e-paper and demonstrates partial refresh. |
| `tx`      | Transmits an incrementing LoRa packet every ~3s, mirrors status to the display. |
| `rx`      | Receives LoRa packets, prints them with RSSI/SNR, shows them on the display. |
| `ble`     | Advertises as `T3S3-Msg` over BLE and accepts text messages over a Nordic UART Service (see below). |

## Toolchain setup

The ESP32-S3 here is the **Xtensa** core, which needs Espressif's Rust fork (the
mainline `rustc` can't target `xtensa-esp32s3-none-elf`). This is what a fresh
machine needs.

### 1. The Xtensa Rust toolchain (`espup`)

```sh
# install espup (downloads a prebuilt binary into ~/.cargo/bin)
curl -L https://github.com/esp-rs/espup/releases/latest/download/espup-aarch64-apple-darwin \
  -o ~/.cargo/bin/espup && chmod +x ~/.cargo/bin/espup
# or: cargo install espup     (compiles from source, slower)

# install the 'esp' toolchain: Xtensa Rust + LLVM + GCC (~1-2 GB download)
espup install
```

`espup install` writes `~/export-esp.sh`. **Source it in every shell** before
building, because the build needs the Xtensa LLVM/clang on `PATH`:

```sh
. ~/export-esp.sh
```

`rust-toolchain.toml` pins `channel = "esp"`, so once installed `cargo` picks it
up automatically inside this repo.

### 2. The flasher (`espflash`)

```sh
# prebuilt binary:
curl -L https://github.com/esp-rs/espflash/releases/latest/download/espflash-aarch64-apple-darwin.zip \
  -o /tmp/espflash.zip && unzip -o /tmp/espflash.zip -d ~/.cargo/bin
# or: cargo install espflash   /   brew install espflash
```

`.cargo/config.toml` sets `runner = "espflash flash --monitor"`, so
`cargo run --example <name>` builds, flashes, and opens the serial monitor.

### 3. Build settings (`.cargo/config.toml`)

Two settings there matter for the `ble` example:

- `build-std = ["core", "alloc"]` — the BLE radio stack (esp-radio →
  esp-alloc/allocator-api2) uses the `alloc` crate. The LoRa/display examples
  only need `core` but are unaffected.
- `ESP_RTOS_CONFIG_TICK_RATE_HZ = "1000"` — raises the esp-rtos scheduler tick
  from its 100 Hz default to 1000 Hz so the radio controller thread is serviced
  on time and BLE advertising/connections are responsive. (esp-config reads this
  from the environment at build time; after changing it run `cargo clean -p
  esp-rtos` to force a rebuild.)

## Build, flash, monitor

```sh
. ~/export-esp.sh
cargo build --release --example ble    # build only
cargo run   --release --example ble    # build + flash + serial monitor
```

Use `--release` for the `ble` example: esp-radio's scheduling is timing-sensitive
and benefits from optimization.

If `cargo run` can't find the board, plug it in over USB and check it enumerates
(`ls /dev/cu.usb*` — a `usbmodem`/`usbserial` device should appear). The T3-S3
has a native USB-JTAG/serial peripheral, so no external USB-UART driver is needed
on recent macOS. If the port is busy, close other serial monitors first.

## BLE messaging (`ble` example + `~/src/ble/ble.py`)

The `ble` example turns the board into a BLE peripheral exposing the **Nordic UART
Service (NUS)** — a de-facto "serial over BLE" layout that generic tools
recognise:

- Service `6e400001-…`
- **RX** `6e400002-…` (`write`): central → board. Every write is printed on the
  serial monitor.
- **TX** `6e400003-…` (`notify`): board → central. The board echoes whatever it
  receives, so the sender gets an ack.

`~/src/ble/ble.py` is a [`uv`](https://docs.astral.sh/uv/) single-file script (BLE
via `bleak`) that scans, dumps GATT tables, and sends messages:

```sh
cd ~/src/ble

# scan for advertising devices, sorted by signal strength
uv run ble.py

# dump the GATT table of every connectable device
uv run ble.py --gatt

# send a message to the board (finds it by advertised name "T3S3-Msg")
uv run ble.py --send "hello from my laptop"

# target a specific device explicitly
uv run ble.py --send "ping" --name T3S3-Msg
uv run ble.py --send "ping" --address AA:BB:CC:DD:EE:FF
```

End-to-end: `cargo run --release --example ble` in one terminal (watch the
monitor), then `uv run ble.py --send "hi"` in another. The board's monitor prints
`ble: received message: "hi" (2 bytes)` and the Python side prints the echo.

### Firmware design notes

- The HCI pump (`runner.run()`) runs as its own `#[embassy_executor::task]`
  (`ble_runner`), with the stack and host resources held in `StaticCell`s so
  they're `'static`. Keeping HCI servicing off the advertise/GATT task is what
  keeps the connection handshake reliable.
- Don't block the embassy executor. The e-paper refresh is ~2 s of blocking SPI;
  if you ever render received messages on the display, do it off the executor so
  it can't stall the radio.

### macOS notes

- The first BLE run prompts for Bluetooth permission — grant your terminal app
  access under System Settings → Privacy & Security → Bluetooth.
- If `--send` reports the board wasn't found while it's clearly advertising,
  CoreBluetooth's cache is stale. Reset the Bluetooth stack and retry:
  `brew install blueutil && blueutil -p 0 && sleep 3 && blueutil -p 1` (or toggle
  Bluetooth in System Settings).

## BLE stack versions (matched set)

The BLE dependencies are a **matched set**, not independently upgradeable: the
controller half (esp-radio) and the host half (trouble-host) bridge through
`bt-hci`, and `ExternalController<BleConnector>` only satisfies trouble-host's
`Controller` trait when both compile against the *same* `bt-hci`.

| Crate          | Version | Note |
|----------------|---------|------|
| `esp-hal`      | 1.1     | board HAL. |
| `esp-radio`    | 0.18    | BLE controller (`BleConnector`); targets esp-hal ~1.1; `bt-hci` 0.8. |
| `esp-rtos`     | 0.3     | scheduler + embassy executor; provides `#[esp_rtos::main]`. |
| `trouble-host` | 0.6     | GATT host; `bt-hci` 0.8 — must match esp-radio. |
| `bt-hci`       | 0.8     | the HCI version both halves agree on. |
| `heapless`     | 0.9     | must match trouble-host's, or its `AsGatt` impl for `Vec<u8, N>` won't apply. |
| `embassy-sync` | 0.7     | the `gatt_server`/`gatt_service` macros expand to `embassy_sync::` paths, so it's a direct dep. |
| `embassy-executor` / `embassy-time` | 0.10 / 0.5 | what esp-rtos 0.3 expects. |

API notes for this generation: `BleConnector::new(BT, Config)` takes two args
(there is no `esp_radio::init()`), and `esp_rtos::start(timer, sw_int)` needs a
`SoftwareInterrupt0` on Xtensa as well as RISC-V.
