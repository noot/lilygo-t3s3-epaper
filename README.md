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
as a serial device. The T3-S3 has a native USB-JTAG/serial peripheral, so no
external USB-UART driver is needed. If the port is busy, close other serial
monitors first.

## BLE ⇄ LoRa bridge (`ble` example)

The `ble` example bridges a BLE central and the LoRa radio, mirroring both
directions to the e-paper. It exposes a Nordic UART Service, so generic BLE
tools work; drive it from a host with `tools/ble.py` (a [`uv`](https://docs.astral.sh/uv/)
single-file script):

```sh
uv run tools/ble.py              # scan
uv run tools/ble.py --send "hi"  # send one message, print the echo
uv run tools/ble.py --listen     # print TX notifications (LoRa receipts + echoes)
uv run tools/ble.py --interact   # REPL
```

See the module docs in `examples/ble.rs` for the service UUIDs, the dual-core
task placement, the continuous-receive LoRa loop, and the matched BLE dependency
set.

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
