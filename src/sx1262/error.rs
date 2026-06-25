//! Error type for the SX1262 driver.

/// An error returned by the SX1262 driver.
///
/// Generic over the SPI device error `S` and the GPIO error `P` so the caller's
/// concrete `embedded-hal` error types are preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<S, P> {
    /// The SPI transfer failed.
    Spi(S),
    /// A GPIO operation failed.
    Pin(P),
    /// The radio did not become ready or signal completion in time.
    Timeout,
    /// The provided buffer was too small or the payload exceeded 255 bytes.
    BufferTooSmall,
    /// A packet was received but failed its CRC check.
    CrcMismatch,
}
