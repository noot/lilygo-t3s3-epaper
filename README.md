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
| `ble`     | BLE ⇄ LoRa bridge with an e-paper mirror: messages cross between a BLE central and the LoRa radio, both directions shown on the display (see below). |
| `wifi_lora_bridge` | Wi-Fi ⇄ LoRa bridge with a web UI: the board hosts an open AP (`lora-tx`) and a small page to send/receive LoRa packets (see below). |

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

## BLE ⇄ LoRa bridge (`ble` example + `tools/ble.py`)

The `ble` example bridges a BLE central and the LoRa radio, mirroring both
directions to the e-paper:

- **BLE → LoRa:** a message written over BLE is shown on the display,
  transmitted over LoRa, and echoed back to the central as an ack.
- **LoRa → BLE:** a packet received over LoRa is shown on the display and pushed
  to the connected central as a notification.

The BLE side is a **Nordic UART Service (NUS)** — a de-facto "serial over BLE"
layout that generic tools recognise:

- Service `6e400001-…`
- **RX** `6e400002-…` (`write`): central → board (forwarded to LoRa).
- **TX** `6e400003-…` (`notify`): board → central (BLE echo + LoRa receipts).

`tools/ble.py` is a [`uv`](https://docs.astral.sh/uv/) single-file script (BLE via
`bleak`) for driving the bridge. It works on Linux (BlueZ), macOS (CoreBluetooth),
and Windows. Run it with `uv run` (which installs `bleak` for you), or
`pip install "bleak>=3,<4"` then `python tools/ble.py`.

```sh
# scan for advertising devices, sorted by signal strength
uv run tools/ble.py

# scan, then dump each connectable device's GATT table
uv run tools/ble.py --gatt

# send one message, print the echo, and exit
uv run tools/ble.py --send "hello from my laptop"

# connect and print TX notifications (watch LoRa receipts + echoes)
uv run tools/ble.py --listen

# REPL: type a line to send it, see replies inline
uv run tools/ble.py --interact

# target a specific device (macOS: a CoreBluetooth UUID; Linux/Windows: a MAC)
uv run tools/ble.py --send "ping" --name T3S3-Msg
uv run tools/ble.py --send "ping" --address AA:BB:CC:DD:EE:FF
```

End-to-end: `cargo run --release --example ble` in one terminal (watch the
monitor), then `uv run tools/ble.py --send "hi"` in another. The board's monitor
prints `ble: received message: "hi" (2 bytes)` and `lora: transmitted 2 bytes`,
and the Python side prints the echo.

### Firmware design notes (task placement)

The blocking work (LoRa SPI, and the ~2 s e-paper refresh) must never stall the
BLE host, so the example splits work across the two cores:

- **Core 0 — BLE host** on the embassy executor. The HCI pump (`runner.run()`)
  runs as its own `#[embassy_executor::task]` (`ble_runner`), with the stack and
  host resources in `StaticCell`s so they're `'static`. Keeping HCI servicing off
  the advertise/GATT task keeps the connection handshake reliable.
- **Core 1 — LoRa + e-paper** (`lora_display_loop`, started via
  `esp_rtos::start_second_core`). It owns both SPI buses and runs a blocking
  loop, so a slow refresh or a LoRa transfer can't touch core 0.
- The two cores exchange fixed-size messages over a pair of `embassy-sync`
  channels (`CriticalSectionRawMutex`, multi-core safe), used with the
  non-blocking `try_send`/`try_receive` so neither core waits on the other. The
  GATT loop polls the LoRa→BLE channel on a 50 ms tick alongside GATT events.
- LoRa receive is bounded (`Sx1262::receive_with_timeout`) so core 1 stops
  listening periodically to drain pending BLE→LoRa transmits (half-duplex radio).

### Host notes

- In a noisy 2.4 GHz environment the connect occasionally times out before the
  link is up; `tools/ble.py` retries the connect 3× and a later attempt almost
  always succeeds. Once connected, the transfer itself is reliable.
- **macOS:** the first BLE run prompts for Bluetooth permission — grant your
  terminal app access under System Settings → Privacy & Security → Bluetooth. If
  the board stops being discovered while it's clearly advertising, CoreBluetooth's
  cache is stale; reset the stack with
  `brew install blueutil && blueutil -p 0 && sleep 3 && blueutil -p 1` (a full
  `sudo killall bluetoothd` is the surest reset).
- **Linux:** ensure BlueZ is running (`systemctl status bluetooth`), the adapter
  is unblocked and powered (`rfkill unblock bluetooth`, `bluetoothctl power on`),
  and your user can access it. `bluetoothctl power off`/`on` clears stale state.

## Wi-Fi ⇄ LoRa bridge (`wifi_lora_bridge` example)

An alternative bridge that fronts the LoRa radio with Wi-Fi instead of BLE:

```sh
cargo run --release --example wifi_lora_bridge
```

The board hosts an open Wi-Fi access point (SSID `lora-tx`) and a small web page.
Join the network from a phone (the captive portal should open), type a message and
send it out over LoRa; incoming LoRa packets are listed live on the page and shown
on the e-paper. The AP bring-up, a minimal DHCP server, and the HTTP server all
live in the example, driving `smoltcp` directly.

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
