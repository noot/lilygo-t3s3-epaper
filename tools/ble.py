# /// script
# requires-python = ">=3.12"
# dependencies = ["bleak>=3,<4"]
# ///
"""Debug and interact with the LilyGO T3-S3 running the `ble` bridge example.

The firmware exposes a Nordic UART Service (NUS): writes to the RX characteristic
are forwarded to LoRa and echoed back, and LoRa receipts are delivered as TX
notifications. This tool can scan, dump GATT tables, send one message, watch
notifications, or open an interactive session.

    uv run ble.py                       # scan, sorted by signal strength
    uv run ble.py --gatt                # scan + dump each device's GATT table
    uv run ble.py --send "hello"        # send one message, print the echo
    uv run ble.py --listen              # connect and print TX notifications
    uv run ble.py --interact            # REPL: type lines to send, see replies

Target selection (for --send/--listen/--interact):
    --name T3S3-Msg                     # advertised name (default)
    --address <addr>                    # exact address; on macOS this is a
                                        # CoreBluetooth UUID, on Linux a MAC.

Cross-platform: works anywhere bleak does (Linux/BlueZ, macOS/CoreBluetooth,
Windows). Run via `uv run ble.py` (uv installs bleak), or `pip install "bleak>=3,<4"`
and `python ble.py`.
"""
import argparse
import asyncio
import sys

from bleak import BleakClient, BleakScanner
from bleak.exc import BleakError

# nordic uart service, as exposed by the `ble` example firmware. rx is the
# characteristic the peripheral accepts writes on; tx is the one it notifies on.
NUS_SERVICE = "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
NUS_RX_CHAR = "6e400002-b5a3-f393-e0a9-e50e24dcca9e"
NUS_TX_CHAR = "6e400003-b5a3-f393-e0a9-e50e24dcca9e"

DEFAULT_DEVICE_NAME = "T3S3-Msg"


def bluetooth_help() -> str:
    """Platform-specific hint for when the adapter is unavailable."""
    if sys.platform == "darwin":
        return ("On macOS, grant your terminal app Bluetooth access under "
                "System Settings -> Privacy & Security -> Bluetooth.")
    if sys.platform.startswith("linux"):
        return ("On Linux, make sure BlueZ is running (systemctl status bluetooth), "
                "the adapter is unblocked and powered (rfkill unblock bluetooth; "
                "bluetoothctl power on), and your user may access it.")
    if sys.platform.startswith("win"):
        return "On Windows, ensure Bluetooth is turned on in Settings."
    return ""


async def enumerate_gatt(device, adv, connect_timeout: float) -> None:
    name = adv.local_name or device.name or "(unknown)"
    print(f"\n{'=' * 90}\nGATT for {device.address}  {name}")

    try:
        async with BleakClient(device, timeout=connect_timeout) as client:
            for service in client.services:
                print(f"  [service] {service.uuid}  {service.description}")
                for char in service.characteristics:
                    props = ",".join(char.properties)
                    print(f"    [char] {char.uuid}  ({props})  {char.description}")
                    for desc in char.descriptors:
                        print(f"      [desc] {desc.uuid}  (handle {desc.handle})")
    except BleakError as e:
        # Refused connection, not connectable, or dropped mid-handshake. Common
        # for beacons and devices already bonded elsewhere — just skip them.
        print(f"  skipped: {e}")
    except asyncio.TimeoutError:
        print(f"  skipped: connect timed out after {connect_timeout:.0f}s")


async def scan(duration: float, gatt: bool, connect_timeout: float) -> int:
    print(f"Scanning for {duration:.0f}s...\n")

    try:
        # return_adv=True gives us {address: (device, advertisement_data)}
        discovered = await BleakScanner.discover(timeout=duration, return_adv=True)
    except BleakError as e:
        # As of bleak 2.0, a powered-off adapter or denied permission raises
        # here (a BleakError subclass) rather than returning an empty result.
        print(f"Bluetooth unavailable: {e}\n{bluetooth_help()}")
        return 1

    if not discovered:
        print("No devices found. Nothing was advertising during the scan "
              "window — try a longer --time.")
        return 0

    # Sort by RSSI, strongest (closest to 0) first.
    results = sorted(discovered.values(), key=lambda pair: pair[1].rssi, reverse=True)

    print(f"{'ADDRESS':<38} {'RSSI':>5}  {'NAME':<22} SERVICES")
    print("-" * 90)
    for device, adv in results:
        name = adv.local_name or device.name or "(unknown)"
        services = ", ".join(adv.service_uuids) if adv.service_uuids else "-"
        print(f"{device.address:<38} {adv.rssi:>5}  {name:<22} {services}")

    print(f"\n{len(results)} device(s) found.")

    if gatt:
        print(f"\nEnumerating GATT tables (connect timeout {connect_timeout:.0f}s)...")
        for device, adv in results:
            await enumerate_gatt(device, adv, connect_timeout)
    return 0


async def find_device(name: str | None, address: str | None, duration: float):
    """Locate the target device by address (exact) or by advertised name."""
    if address:
        print(f"Looking for {address}...")
        device = await BleakScanner.find_device_by_address(address, timeout=duration)
        if device is None:
            print(f"No device with address {address} seen in {duration:.0f}s.")
        return device

    print(f"Looking for a device named {name!r}...")
    device = await BleakScanner.find_device_by_name(name, timeout=duration)
    if device is None:
        print(f"No device named {name!r} seen in {duration:.0f}s. Is the board "
              "powered on and running the `ble` example?")
    return device


def make_notify_handler():
    """A TX-notification printer that decodes UTF-8 where possible."""
    def on_notify(_char, data: bytearray) -> None:
        raw = bytes(data)
        try:
            print(f"  <- {raw.decode('utf-8')!r} ({len(raw)} bytes)")
        except UnicodeDecodeError:
            print(f"  <- {len(raw)} bytes (binary): {raw!r}")
    return on_notify


async def run_with_connection(device, connect_timeout: float, attempts: int, action) -> int:
    """Connect (retrying on the ESP32-S3's occasional handshake miss) and run
    `action(client)`. Returns its int result, or 1 on persistent failure."""
    for attempt in range(1, attempts + 1):
        print(f"Connecting to {device.address} (attempt {attempt}/{attempts})...")
        try:
            disconnected = asyncio.Event()
            async with BleakClient(
                device,
                timeout=connect_timeout,
                disconnected_callback=lambda _c: disconnected.set(),
            ) as client:
                print("  connected")
                return await action(client, disconnected)
        except (BleakError, asyncio.TimeoutError) as e:
            reason = "connect timed out" if isinstance(e, asyncio.TimeoutError) else str(e)
            if attempt < attempts:
                print(f"  {reason}; retrying...")
            else:
                print(f"Failed after {attempts} attempts: {reason}")
                return 1
    return 1


def action_send(message: str):
    async def _action(client, _disconnected) -> int:
        echo = asyncio.Event()

        def on_notify(_char, data: bytearray) -> None:
            print(f"  <- echo: {bytes(data).decode('utf-8', 'replace')!r}")
            echo.set()

        try:
            await client.start_notify(NUS_TX_CHAR, on_notify)
        except BleakError:
            pass  # notifications are a bonus; the write still works.

        payload = message.encode("utf-8")
        print(f"  -> writing {len(payload)} bytes to RX characteristic")
        await client.write_gatt_char(NUS_RX_CHAR, payload, response=True)
        try:
            await asyncio.wait_for(echo.wait(), timeout=2.0)
        except asyncio.TimeoutError:
            print("  (no echo received, but the write was acknowledged)")
        print("Done.")
        return 0
    return _action


def action_listen():
    async def _action(client, disconnected) -> int:
        await client.start_notify(NUS_TX_CHAR, make_notify_handler())
        print("Listening for TX notifications (Ctrl-C to stop)...")
        await disconnected.wait()
        print("Device disconnected.")
        return 0
    return _action


def action_interact():
    async def _action(client, disconnected) -> int:
        await client.start_notify(NUS_TX_CHAR, make_notify_handler())
        print("Interactive session. Type a line and press Enter to send it; "
              "Ctrl-D or Ctrl-C to quit.")
        loop = asyncio.get_running_loop()
        disc_wait = asyncio.ensure_future(disconnected.wait())
        try:
            while not disconnected.is_set():
                # read stdin off the event loop so notifications keep printing.
                read = asyncio.ensure_future(loop.run_in_executor(None, sys.stdin.readline))
                await asyncio.wait({read, disc_wait}, return_when=asyncio.FIRST_COMPLETED)
                if disc_wait.done():
                    read.cancel()
                    print("Device disconnected.")
                    break
                line = read.result()
                if line == "":  # EOF (Ctrl-D)
                    break
                text = line.rstrip("\n")
                if not text:
                    continue
                try:
                    await client.write_gatt_char(NUS_RX_CHAR, text.encode("utf-8"), response=True)
                except BleakError as e:
                    print(f"  write failed: {e}")
                    break
        finally:
            disc_wait.cancel()
        return 0
    return _action


async def interact(action, name, address, duration, connect_timeout) -> int:
    try:
        device = await find_device(name, address, duration)
    except BleakError as e:
        print(f"Bluetooth unavailable: {e}\n{bluetooth_help()}")
        return 1
    if device is None:
        return 1
    return await run_with_connection(device, connect_timeout, 3, action)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Scan for, send to, or interact with the T3-S3 BLE bridge.",
    )
    parser.add_argument(
        "-t", "--time", type=float, default=10.0,
        help="Scan / discovery duration in seconds (default: 10)",
    )
    parser.add_argument(
        "-g", "--gatt", action="store_true",
        help="Scan, then dump the GATT table of every connectable device",
    )
    parser.add_argument(
        "--connect-timeout", type=float, default=10.0,
        help="Per-connection timeout in seconds (default: 10)",
    )
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "-s", "--send", metavar="TEXT",
        help="Send TEXT to the board, print the echo, and exit",
    )
    mode.add_argument(
        "-l", "--listen", action="store_true",
        help="Connect and print TX notifications until the device disconnects",
    )
    mode.add_argument(
        "-i", "--interact", action="store_true",
        help="Connect and open a REPL: type lines to send, see replies",
    )
    parser.add_argument(
        "--name", default=DEFAULT_DEVICE_NAME,
        help=f"Advertised name of the target device (default: {DEFAULT_DEVICE_NAME})",
    )
    parser.add_argument(
        "--address",
        help="Target by address (macOS: a CoreBluetooth UUID; Linux/Windows: a MAC)",
    )
    args = parser.parse_args()

    try:
        if args.send is not None:
            rc = asyncio.run(interact(
                action_send(args.send), args.name, args.address, args.time, args.connect_timeout,
            ))
        elif args.listen:
            rc = asyncio.run(interact(
                action_listen(), args.name, args.address, args.time, args.connect_timeout,
            ))
        elif args.interact:
            rc = asyncio.run(interact(
                action_interact(), args.name, args.address, args.time, args.connect_timeout,
            ))
        else:
            rc = asyncio.run(scan(args.time, args.gatt, args.connect_timeout))
    except KeyboardInterrupt:
        rc = 0
    raise SystemExit(rc)


if __name__ == "__main__":
    main()
