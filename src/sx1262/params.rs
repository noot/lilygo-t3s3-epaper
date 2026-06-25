//! Radio configuration types for the SX1262 LoRa modem.

/// LoRa spreading factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadingFactor {
    Sf5,
    Sf6,
    Sf7,
    Sf8,
    Sf9,
    Sf10,
    Sf11,
    Sf12,
}

impl SpreadingFactor {
    pub(crate) fn reg(self) -> u8 {
        match self {
            Self::Sf5 => 5,
            Self::Sf6 => 6,
            Self::Sf7 => 7,
            Self::Sf8 => 8,
            Self::Sf9 => 9,
            Self::Sf10 => 10,
            Self::Sf11 => 11,
            Self::Sf12 => 12,
        }
    }
}

/// LoRa signal bandwidth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bandwidth {
    Bw125kHz,
    Bw250kHz,
    Bw500kHz,
}

impl Bandwidth {
    pub(crate) fn reg(self) -> u8 {
        match self {
            Self::Bw125kHz => 0x04,
            Self::Bw250kHz => 0x05,
            Self::Bw500kHz => 0x06,
        }
    }

    pub(crate) fn hz(self) -> u32 {
        match self {
            Self::Bw125kHz => 125_000,
            Self::Bw250kHz => 250_000,
            Self::Bw500kHz => 500_000,
        }
    }
}

/// LoRa forward-error-correction coding rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodingRate {
    Cr4_5,
    Cr4_6,
    Cr4_7,
    Cr4_8,
}

impl CodingRate {
    pub(crate) fn reg(self) -> u8 {
        match self {
            Self::Cr4_5 => 0x01,
            Self::Cr4_6 => 0x02,
            Self::Cr4_7 => 0x03,
            Self::Cr4_8 => 0x04,
        }
    }
}

/// TCXO control voltage supplied from the radio's DIO3 pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcxoVoltage {
    V1_6,
    V1_7,
    V1_8,
    V2_2,
    V2_4,
    V2_7,
    V3_0,
    V3_3,
}

impl TcxoVoltage {
    pub(crate) fn reg(self) -> u8 {
        match self {
            Self::V1_6 => 0x00,
            Self::V1_7 => 0x01,
            Self::V1_8 => 0x02,
            Self::V2_2 => 0x03,
            Self::V2_4 => 0x04,
            Self::V2_7 => 0x05,
            Self::V3_0 => 0x06,
            Self::V3_3 => 0x07,
        }
    }
}

/// Radio configuration applied during [`crate::sx1262::Sx1262::init`].
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Centre frequency in Hz.
    pub frequency_hz: u32,
    pub spreading_factor: SpreadingFactor,
    pub bandwidth: Bandwidth,
    pub coding_rate: CodingRate,
    /// Preamble length in symbols.
    pub preamble_len: u16,
    /// Transmit power in dBm (-9..=22 for the SX1262).
    pub tx_power_dbm: i8,
    /// Append/check a CRC on the payload.
    pub crc_on: bool,
    /// Use a variable-length (explicit) header rather than a fixed-length one.
    pub explicit_header: bool,
    /// Invert I and Q signals.
    pub invert_iq: bool,
    /// Use the internal DC-DC regulator rather than the LDO.
    pub use_dcdc: bool,
    /// Enable the boosted receive gain.
    pub rx_boost: bool,
    /// TCXO supply voltage, or `None` for a board without a TCXO.
    pub tcxo_voltage: Option<TcxoVoltage>,
}

impl Default for Config {
    /// Sensible defaults for the T3-S3 e-paper board: 915 MHz, SF7, 125 kHz,
    /// 4/5, +22 dBm, explicit header with CRC, TCXO at 1.8 V via DIO3.
    fn default() -> Self {
        Self {
            frequency_hz: crate::board::DEFAULT_FREQUENCY_HZ,
            spreading_factor: SpreadingFactor::Sf7,
            bandwidth: Bandwidth::Bw125kHz,
            coding_rate: CodingRate::Cr4_5,
            preamble_len: 8,
            tx_power_dbm: 22,
            crc_on: true,
            explicit_header: true,
            invert_iq: false,
            use_dcdc: true,
            rx_boost: true,
            tcxo_voltage: Some(crate::board::TCXO_VOLTAGE),
        }
    }
}

impl Config {
    /// Convert the centre frequency to the 32-bit PLL value expected by
    /// `SetRfFrequency`: `freq * 2^25 / 32_000_000`.
    pub(crate) fn frequency_pll(&self) -> u32 {
        const XTAL_HZ: u64 = 32_000_000;
        (((self.frequency_hz as u64) << 25) / XTAL_HZ) as u32
    }

    /// Image-calibration band bytes for the configured frequency (datasheet
    /// `CalibrateImage` table).
    pub(crate) fn calibrate_image_band(&self) -> [u8; 2] {
        match self.frequency_hz {
            f if (902_000_000..=928_000_000).contains(&f) => [0xE1, 0xE9],
            f if (863_000_000..=870_000_000).contains(&f) => [0xD7, 0xDB],
            f if (779_000_000..=787_000_000).contains(&f) => [0xC1, 0xC5],
            f if (470_000_000..=510_000_000).contains(&f) => [0x75, 0x81],
            // default to the 430-440 MHz band
            _ => [0x6B, 0x6F],
        }
    }

    /// Low-data-rate optimization is required when symbol duration exceeds
    /// 16.38 ms (datasheet 6.1.4).
    pub(crate) fn low_data_rate_optimize(&self) -> u8 {
        let symbol_us =
            ((1u64 << self.spreading_factor.reg()) * 1_000_000) / self.bandwidth.hz() as u64;
        u8::from(symbol_us > 16_380)
    }
}
