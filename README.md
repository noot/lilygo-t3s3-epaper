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
mainline `rustc` can't target `xtensa-esp32s3-none-elf`). Everything below is what
a fresh machine needs.

### 1. The Xtensa Rust toolchain (`espup`)

```sh
# install espup (downloads a prebuilt binary into ~/.cargo/bin)
curl -L https://github.com/esp-rs/espup/releases/latest/download/espup-aarch64-apple-darwin \
  -o ~/.cargo/bin/espup && chmod +x ~/.cargo/bin/espup
# or: cargo install espup     (compiles from source, slower)

# install the 'esp' toolchain: Xtensa Rust + LLVM + GCC (~1-2 GB download)
espup install
```

`espup install` writes `~/export-esp.sh`. **You must source it in every shell**
before building, because the build needs the Xtensa LLVM/clang on `PATH`:

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
# or: cargo install espflash
#  or: brew install espflash
```

`.cargo/config.toml` sets `runner = "espflash flash --monitor"`, so
`cargo run --example <name>` builds, flashes, and opens the serial monitor.

### 3. `build-std`

`.cargo/config.toml` builds the `core` **and `alloc`** sysroot crates from source
(`build-std = ["core", "alloc"]`). `alloc` is required by the `ble` example's
radio stack (esp-radio pulls `esp-alloc`/`allocator-api2`, which `use`s the
`alloc` crate). The LoRa/display examples only need `core` but are unaffected.

## Build, flash, monitor

```sh
. ~/export-esp.sh
cargo build --example ble            # build only
cargo run   --example ble            # build + flash + serial monitor
```

If `cargo run` can't find the board, plug it in over USB and check it enumerates:

```sh
ls /dev/cu.usb*        # macOS: a usbmodem/usbserial device should appear
```

The T3-S3 has a native USB-JTAG/serial peripheral, so no external USB-UART driver
is needed on recent macOS. If the port is busy, close other serial monitors first.

## BLE messaging demo (`ble` example + `~/src/ble/ble.py`)

The `ble` example turns the board into a BLE peripheral exposing the **Nordic UART
Service (NUS)** — a de-facto "serial over BLE" layout that generic tools recognise:

- Service `6e400001-…`
- **RX** `6e400002-…` (`write`): central → board. Every write is printed on the
  serial monitor.
- **TX** `6e400003-…` (`notify`): board → central. The board echoes whatever it
  receives, so the sender gets an ack.

`~/src/ble/ble.py` is a [`uv`](https://docs.astral.sh/uv/) single-file script
(BLE via `bleak`). It scans, dumps GATT tables, and now sends messages:

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

End-to-end: `cargo run --example ble` in one terminal (watch the monitor), then
`uv run ble.py --send "hi"` in another. The board's monitor prints
`ble: received message: "hi" (2 bytes)` and the Python side prints the echo.

> **macOS:** the first BLE run prompts for Bluetooth permission. Grant your
> terminal app access under System Settings → Privacy & Security → Bluetooth.

## BLE stack version-matching (read before bumping deps)

The BLE dependencies in `Cargo.toml` are a **matched set**, not independently
upgradeable. The two halves of the stack bridge through the `bt-hci` crate, and
the `ExternalController<BleConnector>` only satisfies trouble-host's `Controller`
trait if both sides compile against the *same* `bt-hci`:

| Crate          | Version | Notes |
|----------------|---------|-------|
| `esp-hal`      | 1.1     | the board HAL. |
| `esp-radio`    | 0.18    | BLE controller (`BleConnector`); targets esp-hal ~1.1, uses `bt-hci` 0.8. |
| `esp-rtos`     | 0.3     | scheduler + embassy executor; provides `#[esp_rtos::main]`. |
| `trouble-host` | 0.6     | GATT host; uses `bt-hci` 0.8 — **must match esp-radio**. |
| `bt-hci`       | 0.8     | the HCI bridge both halves agree on. |
| `heapless`     | 0.9     | **must match trouble-host's** — its `AsGatt` impl for `Vec<u8, N>` is version-specific, so a mismatched heapless gives "trait not implemented". |
| `embassy-sync` | 0.7     | the `gatt_server`/`gatt_service` macros expand to `embassy_sync::` paths, so it's a direct dep. |
| `embassy-executor` / `embassy-time` | 0.10 / 0.5 | what esp-rtos 0.3 expects. |

Gotchas hit while wiring this up, in case you bump versions:

- **`esp_radio::init()` doesn't exist in 0.18.** Scheduling comes from
  `esp_rtos::start(timer, sw_int)` and `BleConnector::new(BT, Config)` takes only
  two args. Older tutorials show a 3-arg `BleConnector::new(&radio, BT, …)`.
- **`esp_rtos::start` needs a `SoftwareInterrupt0` on Xtensa too**, not just
  RISC-V. Older (0.2) examples gate that argument behind `cfg(riscv32)`.
- A characteristic whose value type isn't `Copy` (e.g. `Vec<u8, N>`) can't be
  copied out of `&server.…`; borrow it (`let rx = &server.nus.rx;`).

## Scheduler tick rate (don't starve the radio)

esp-rtos defaults to a **100 Hz** scheduler tick (10 ms time-slices). The
esp-radio BLE controller runs as its own RTOS thread; with 10 ms preemption
granularity it can't service the link layer often enough, so the board
advertises sparsely and a scanner can take *minutes* to see it (or miss it
entirely in a short scan window).

`.cargo/config.toml` raises it to 1000 Hz (1 ms), which is an esp-config
build-time setting read from the environment:

```toml
[env]
ESP_RTOS_CONFIG_TICK_RATE_HZ = "1000"
```

If you change it, you must force esp-rtos to recompile (cargo doesn't always pick
up the env change on its own): `cargo clean -p esp-rtos` then rebuild.

Related scheduling rules of thumb for this stack:

- **Build the `ble` example in `--release`** (or at least with optimizations).
  esp-radio's scheduling glue is timing-sensitive; an unoptimized build can miss
  controller deadlines.
- **Never block the embassy executor.** The e-paper refresh is ~2 s of blocking
  SPI — if you render received messages on the display from inside an async task,
  that freeze starves the radio. Do display work on a separate path.
- For heavier setups, spawn `runner.run()` as its own `#[embassy_executor::task]`
  (needs `'static` resources via `StaticCell`) so the HCI pump is scheduled
  independently of GATT/advertise work.

## macOS discovery is slow to surface device names

macOS CoreBluetooth can be slow and cache-happy about surfacing a peripheral's
advertised name, especially in a dense BLE environment. If `--send` reports the
device wasn't found, scan longer (`--time 60`) or scan first with
`uv run ble.py --time 30` and target the address directly with `--send "…"
--address <UUID>`. Toggling the Mac's Bluetooth off/on clears the cache.
