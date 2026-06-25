//! SX1262 SPI opcodes, registers and IRQ bit definitions (datasheet rev. 2.1).

// command opcodes
pub(crate) const SET_STANDBY: u8 = 0x80;
pub(crate) const SET_TX: u8 = 0x83;
pub(crate) const SET_RX: u8 = 0x82;
pub(crate) const SET_REGULATOR_MODE: u8 = 0x96;
pub(crate) const CALIBRATE: u8 = 0x89;
pub(crate) const CALIBRATE_IMAGE: u8 = 0x98;
pub(crate) const SET_PA_CONFIG: u8 = 0x95;
pub(crate) const WRITE_REGISTER: u8 = 0x0D;
pub(crate) const READ_REGISTER: u8 = 0x1D;
pub(crate) const WRITE_BUFFER: u8 = 0x0E;
pub(crate) const READ_BUFFER: u8 = 0x1E;
pub(crate) const SET_DIO_IRQ_PARAMS: u8 = 0x08;
pub(crate) const GET_IRQ_STATUS: u8 = 0x12;
pub(crate) const CLEAR_IRQ_STATUS: u8 = 0x02;
pub(crate) const SET_DIO2_AS_RF_SWITCH: u8 = 0x9D;
pub(crate) const SET_DIO3_AS_TCXO: u8 = 0x97;
pub(crate) const SET_RF_FREQUENCY: u8 = 0x86;
pub(crate) const SET_PACKET_TYPE: u8 = 0x8A;
pub(crate) const SET_TX_PARAMS: u8 = 0x8E;
pub(crate) const SET_MODULATION_PARAMS: u8 = 0x8B;
pub(crate) const SET_PACKET_PARAMS: u8 = 0x8C;
pub(crate) const SET_BUFFER_BASE_ADDRESS: u8 = 0x8F;
pub(crate) const GET_RX_BUFFER_STATUS: u8 = 0x13;
pub(crate) const GET_PACKET_STATUS: u8 = 0x14;
pub(crate) const CLEAR_DEVICE_ERRORS: u8 = 0x07;

// standby configs
pub(crate) const STDBY_RC: u8 = 0x00;

// packet type
pub(crate) const PACKET_TYPE_LORA: u8 = 0x01;

// regulator mode
pub(crate) const REGULATOR_LDO: u8 = 0x00;
pub(crate) const REGULATOR_DCDC: u8 = 0x01;

// pa ramp time (200 us)
pub(crate) const RAMP_200_US: u8 = 0x04;

// calibration: all blocks
pub(crate) const CALIBRATE_ALL: u8 = 0x7F;

// registers
pub(crate) const REG_RX_GAIN: u16 = 0x08AC;
pub(crate) const REG_OCP: u16 = 0x08E7;
pub(crate) const REG_TX_CLAMP_CONFIG: u16 = 0x08D8;

// register values
pub(crate) const RX_GAIN_BOOSTED: u8 = 0x96;
pub(crate) const OCP_140_MA: u8 = 0x38;

// irq flags
pub(crate) const IRQ_TX_DONE: u16 = 0x0001;
pub(crate) const IRQ_RX_DONE: u16 = 0x0002;
pub(crate) const IRQ_CRC_ERR: u16 = 0x0040;
pub(crate) const IRQ_TIMEOUT: u16 = 0x0200;
pub(crate) const IRQ_ALL: u16 = 0xFFFF;

// pa config for the sx1262 at +22 dBm (datasheet table 13-21)
pub(crate) const PA_CONFIG_SX1262: [u8; 4] = [0x04, 0x07, 0x00, 0x01];
