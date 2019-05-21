// steps to read from I2C
// 1. send the start sequence
// 2. send the address of the device
// 3. send 1 bit to indicate a write
// 4. receive the ACK
// 5. send the byte offset where you want to read from
// 6. receive the ACK
// 7. send the start sequence again
// 8. send the address of the device
// 9. send a bit to indicate read
// 10. recweive the ACK
// 11. read data byte
// 12. send NAK bit
// 13. send close signal

// SFF = Small Form Factor
// SFP = Small Form Factor Pluggable
// SCL = 
// SCD = 

use super::registers::{IntelIxgbeRegisters, SWSM_SMBI, SWSM_SWESMBI, SW_FW_SYNC_FW_MAC, SW_FW_SYNC_SMBITS_FW, SW_FW_SYNC_SMBITS_MASK, SW_FW_SYNC_SMBITS_SW,SW_FW_SYNC_SW_MAC};
use hpet::get_hpet;


///device address
const I2C_EEPROM_DEV_ADDR: u8 = 0xA0;
const I2C_EEPROM_DEV_ADDR2: u8 =  0xA2;

/// byte offsets
pub const SFF_IDENTIFIER: u8 = 0x0;
pub const SFF_1GBE_COMP_CODES: u8 =	0x6;
pub const SFF_10GBE_COMP_CODES: u8 = 0x3;
pub const SFF_CABLE_TECHNOLOGY: u8 =	0x8;

/// values at byte offsets
const SFF_IDENTIFIER_SFP: u8 = 	0x3;

//I2C commands
const I2C_READ: u8 = 1;
const I2C_WRITE: u8 = 0;

// bit offsets for Data and Clk in I2CCTL register
const IXGBE_I2C_CLK_IN: u32 = 1 << 0;
const IXGBE_I2C_CLK_OUT: u32 = 1 << 1;
const IXGBE_I2C_DATA_IN: u32 = 1 << 2;
const IXGBE_I2C_DATA_OUT: u32 = 1 << 3;

//wait times in uS for standard I2C operation
/// clock frequency (kHz) in standard I2C mode
const I2C_F_SCL: u32 = 100;
/// Hold time (repeated START condition)
const I2C_T_HD_START: u32 = 4;
/// Low period of the SCL clock
const I2C_T_LOW: u32 = 5; //actually 4.7
/// High period of the SCL clock
const I2C_T_HIGH: u32 = 4;
/// Set up time for a repeated Start condition
const I2C_T_SU_START: u32 = 5; // actually 4.7
/// Data hold time
const I2C_T_HD_DATA: u32 = 5;
/// Data set up time
const I2C_T_SU_DATA: u32 = 1; // actually 250ns
/// Rise time of both SCA and SDA signals
const I2C_T_RISE: u32 = 1; // actually 1000 ns
/// Fall time of both SCA and SDA signals
const I2C_T_FALL:u32 = 1; //actually 300 ns
//set up time for Stop condition
const I2C_T_SU_STOP: u32 = 4;  
// bus free time between a start and stop condition
const I2C_T_BUF:u32 = 5; // actually 4.7

const I2C_CLOCK_STRETCHING_TIMEOUT: u16 = 500;

const SUCCESS: bool = true;
const FAIL: bool = false;





/// Reads 8 bit EEPROM word over I2C interface
/// byte_offset: EEPROM address to read from
pub fn read_i2c_eeprom(regs: &mut IntelIxgbeRegisters, byte_offset: u8) -> Result<u8, &'static str> {
    let max_retry = 10;
    let mut data = 0;

    let val = regs.i2cctl.read();
    debug!("{}", val);

    regs.i2cctl.write(val & 0);

    let val = regs.i2cctl.read();
    debug!("{}", val);
    

    for _ in 0..max_retry {
        // acquire sw/fw semaphore
        acquire_semaphore(regs)?;
        debug!("Semaphore acquired");

        // start of frame
        let res = i2c_start(regs);
        if res.is_err() {
            debug!("i2c didnt start");
            let _ = i2c_bus_clear(regs);
            continue;
        }
        debug!("i2c started");
        // break;

        //device address and write indication
        let res = clock_out_i2c_byte(regs, I2C_EEPROM_DEV_ADDR2 & !(I2C_WRITE));
        if res.is_err() {
            debug!("failed to send dev address");
            let _ = i2c_bus_clear(regs);
            continue;
        }
        debug!("sent dev address");

        let res = get_i2c_ack(regs);
        if res.is_err() {
            debug!("failed to receive ack");
            let _ = i2c_bus_clear(regs);
            continue;
        }
        debug!("sent dev address and received ack");

        // //byte offset 
        // let _ = clock_out_i2c_byte(regs, byte_offset);
        // let _ = get_i2c_ack(regs);
        // debug!("sent byte offset and received ack");
        
        // let _ = i2c_start(regs);
        // debug!("i2c started");
        
        // //device address and read indication
        // let _ = clock_out_i2c_byte(regs, I2C_EEPROM_DEV_ADDR | I2C_READ);
        // let _ = get_i2c_ack(regs);
        // debug!("sent dev address and received ack");
            
        // // get data
        // data = clock_in_i2c_byte(regs);
        // debug!("received data");
        
        // //send NACK
        // let _ = send_i2c_nack(regs);
        // debug!("sent ack");
        
        // //end of frame
        // let _ = i2c_stop(regs);
        // debug!("sent stop");
    }

    //release semaphore
    Ok(data)
}

fn i2c_bus_clear(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str>{
    i2c_start(regs)?;

    // let i2cctl_val = regs.i2cctl.read();
    set_i2c_data(regs, true)?;

    for _ in 0..=9 {
        raise_i2c_clk(regs);
        let _ = pit_clock::pit_wait(I2C_T_HIGH);
        lower_i2c_clk(regs);
        let _ = pit_clock::pit_wait(I2C_T_LOW);
    }

    i2c_start(regs)?;
    i2c_stop(regs)?;

    Ok(())
}
/// switch SDA from high to low, then the SCL from high to loq
fn i2c_start(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str>{

    //SCL and SDA are both high at the start
    set_i2c_data(regs, true)?;
    raise_i2c_clk(regs);

    // setup time for start condition (4.7us)
    let _ =pit_clock::pit_wait(I2C_T_SU_START);

    // SDA is shifted to low
    set_i2c_data(regs, false)?;

    // hold time for start condition (4us)
    let _ =pit_clock::pit_wait(I2C_T_HD_START);

    lower_i2c_clk(regs);

    // minimum low period of the clock (4.7 us)
    let _ =pit_clock::pit_wait(I2C_T_LOW);
    Ok(())
}


//switch SCL to high then SDA to high
fn i2c_stop (regs: &mut IntelIxgbeRegisters) -> Result <(), &'static str>{
    //starts with data low and clock high
    set_i2c_data(regs, false)?;
    raise_i2c_clk(regs);

    // setup time for stop condition (4us)
    let _ =pit_clock::pit_wait(I2C_T_SU_STOP);

    set_i2c_data(regs, true)?;

    // bus free time between stop and start (4.7 us)
    let _ =pit_clock::pit_wait(I2C_T_BUF);    

    Ok(())
}

fn clock_in_i2c_byte(regs: &mut IntelIxgbeRegisters) -> u8 {
    let mut data = 0;

    for i in (0..=7).rev() {
        let bit = bool_to_u8(clock_in_i2c_bit(regs));
        data |= bit << i;
    }

    data
}

fn bool_to_u8(b: bool) -> u8 {
    if b { 1 }
    else { 0 }
}

fn u8_to_bool(val: u8) -> bool {
    if val == 1 { true }
    else { false }
}

// change SDA to required bit then change clock from high to low
fn clock_out_i2c_byte (regs: &mut IntelIxgbeRegisters, data: u8) -> Result< (), &'static str> {

    for i in (0..=7).rev() {
        let bit = u8_to_bool((data >> i) & 0x1);
        clock_out_i2c_bit(regs, bit)?;
    }

    // release SDA line by setting high
    let i2cctl_val = regs.i2cctl.read() | IXGBE_I2C_DATA_OUT;
    regs.i2cctl.write(i2cctl_val); 
    regs.i2cctl.read();

    Ok(())
}

// should receive 1 low bit
fn get_i2c_ack (regs: &mut IntelIxgbeRegisters) -> Result <(), &'static str> {
    let timeout = 10;

    raise_i2c_clk(regs);

    // Minimum high period of clock is 4us
    let _ =pit_clock::pit_wait(I2C_T_HIGH);

    // poll for ACK - a transition from 1 to 0
    let mut ack = false;
    for _ in 0..timeout {
        ack = get_i2c_data(regs);
        if !ack {
            break;
        }
    }

    lower_i2c_clk(regs);

    // Minimum low period of clock is 4.7us
    let _ =pit_clock::pit_wait(I2C_T_LOW);

    if ack {
        warn!("Ixgbe phy: ACK not received");
        return Err("Ixgbe phy: ACK not received");
    }

    Ok(())    
}

/// A master/receiver is doen reading data and indicates this to slave using a NACK
fn send_i2c_nack(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str> {
    let nack = true;
    clock_out_i2c_bit(regs, nack)?;
    Ok(())
}

fn clock_in_i2c_bit(regs: &mut IntelIxgbeRegisters) -> bool {
    raise_i2c_clk(regs);

    // Minimum high period of clock is 4us
    let _ =pit_clock::pit_wait(I2C_T_HIGH);

    let data = get_i2c_data(regs);

    lower_i2c_clk(regs);

    // Minimum low period of clock is 4.7us
    let _ =pit_clock::pit_wait(I2C_T_LOW);

    data
}

fn clock_out_i2c_bit(regs: &mut IntelIxgbeRegisters, value: bool) -> Result<(), &'static str> {
    debug!("bit:{}", value);
    let status = set_i2c_data(regs, value)?;
    
    raise_i2c_clk(regs);

    // Minimum high period of clock is 4us
    let _ =pit_clock::pit_wait(I2C_T_HIGH);

    lower_i2c_clk(regs);

    // Minimum low period of clock is 4.7 us.
    // This also takes care of the data hold time.
    let _ =pit_clock::pit_wait(I2C_T_LOW);

    Ok(())
}

fn set_i2c_data(regs: &mut IntelIxgbeRegisters, value: bool) -> Result<(), &'static str> {
    let val = regs.i2cctl.read();
    // debug!("set i2c data: {}", val);
    
    let i2cctl_val = 
        if value {
            regs.i2cctl.read() | IXGBE_I2C_DATA_OUT
        }
        else {
            regs.i2cctl.read() & !IXGBE_I2C_DATA_OUT
        };

    // debug!("set i2c data val before write: {}", i2cctl_val);
    regs.i2cctl.write(i2cctl_val);
    regs.i2cctl.read(); //inserted to give time
    // Data rise/fall (1000ns/300ns) and set-up time (250ns)
    let _ =pit_clock::pit_wait(I2C_T_RISE + I2C_T_FALL + I2C_T_SU_DATA);

    // can't check if data was sent correctly if value was 0
    if value == false {
        return Ok(());
    }

    //check if data was sent correctly
    if value != get_i2c_data(regs) {
        warn!("Ixgbe phy:: data not sent!");
        return Err("Ixgbe phy:: data not sent!");
    }
    
    Ok(())
}

fn get_i2c_data(regs: &mut IntelIxgbeRegisters) -> bool {
    // debug!("get i2c data");

    let i2cctl_val = regs.i2cctl.read();
    // debug!("i2c data: {}", i2cctl_val);
    // data is 1
    if i2cctl_val & IXGBE_I2C_DATA_IN == IXGBE_I2C_DATA_IN {
        return true;
    }
    // data is 0
    else {
        return false;
    }
}

fn raise_i2c_clk(regs: &mut IntelIxgbeRegisters) {
    // debug!("raised i2c clock");
    let timeout = I2C_CLOCK_STRETCHING_TIMEOUT;

    for i in 0..timeout{
        let val = regs.i2cctl.read();
        regs.i2cctl.write(val | IXGBE_I2C_CLK_OUT);
        regs.i2cctl.read(); //extra time
        // SCL rise time (1000ns)
        let _ =pit_clock::pit_wait(I2C_T_RISE);

        let val = regs.i2cctl.read();
        if (val & IXGBE_I2C_CLK_IN) == IXGBE_I2C_CLK_IN {
            break;
        }
    }
}

fn lower_i2c_clk(regs: &mut IntelIxgbeRegisters) {
    // debug!("lowered i2c clock");

    let val = regs.i2cctl.read();
    regs.i2cctl.write(val & !IXGBE_I2C_CLK_OUT);
    regs.i2cctl.read(); //extra time

    // SCL fall time (300ns)
    let _ =pit_clock::pit_wait(I2C_T_FALL);
}

/// acquires semaphore to synchronize between software and firmware (section 10.5.4)
    /// used for autoc and autoc2 registers
    fn acquire_semaphore(regs: &mut IntelIxgbeRegisters) -> Result<bool, &'static str> {

        // check that some other sofware is not using the semaphore
        // 1. poll SWSM.SMBI bit until reads as 0 or 10ms timer expires
        let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let period_fs: u64 = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.counter_period_femtoseconds() as u64;
        let fs_per_ms: u64 = 1_000_000_000_000;
        let mut timer_expired_smbi = false;
        let mut smbi_bit = 1;
        while smbi_bit != 0 {
            smbi_bit = regs.swsm.read() & SWSM_SMBI;
            let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

            if (end-start) * period_fs / fs_per_ms == 10 {
                timer_expired_smbi = true;
                break;
            }
        } 
        // now, hardware will auto write 1 to the SMBI bit

        // check that firmware is not using the semaphore
        // 1. write to SWESMBI bit
        let set_swesmbi = regs.swsm.read() | SWSM_SWESMBI; // set bit 1 to 1
        regs.swsm.write(set_swesmbi);

        // 2. poll SWSM.SWESMBI bit until reads as 1 or 3s timer expires
        let start = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();
        let mut swesmbi_bit = 0;
        let mut timer_expired_swesmbi = false;
        while swesmbi_bit == 0 {
            swesmbi_bit = (regs.swsm.read() & SWSM_SWESMBI) >> 1;
            let end = get_hpet().as_ref().ok_or("couldn't get HPET timer")?.get_counter();

            if (end-start) * period_fs / fs_per_ms == 3000 {
                timer_expired_swesmbi = true;
                break;
            }
        } 

        // software takes control of the requested resource
        // 1. read firmware and software bits of sw_fw_sync register 
        let mut sw_fw_sync_smbits = regs.sw_fw_sync.read() & SW_FW_SYNC_SMBITS_MASK;
        let sw_mac = (sw_fw_sync_smbits & SW_FW_SYNC_SW_MAC) >> 3;
        let fw_mac = (sw_fw_sync_smbits & SW_FW_SYNC_FW_MAC) >> 8;

        // clear sw sempahore bits if sw malfunction
        if timer_expired_smbi {
            sw_fw_sync_smbits &= !(SW_FW_SYNC_SMBITS_SW);
        }

        // clear fw semaphore bits if fw malfunction
        if timer_expired_swesmbi {
            sw_fw_sync_smbits &= !(SW_FW_SYNC_SMBITS_FW);
        }

        regs.sw_fw_sync.write(sw_fw_sync_smbits);

        // check if semaphore bits for the resource are cleared
        // then resources are available
        if (sw_mac == 0) && (fw_mac == 0) {
            //claim the sw resource by setting the bit
            let sw_fw_sync = regs.sw_fw_sync.read() & SW_FW_SYNC_SW_MAC;
            regs.sw_fw_sync.write(sw_fw_sync);

            //clear bits in the swsm register
            let swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);
            regs.swsm.write(swsm);

            return Ok(true);
        }

        //resource is not available
        else {
            //clear bits in the swsm register
            let swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);
            regs.swsm.write(swsm);

            Ok(false)
        }
    }

    fn release_semaphore(regs: &mut IntelIxgbeRegisters) -> Result<(), &'static str> {
        // clear bit of released resource
        let sw_fw_sync = regs.sw_fw_sync.read() & !(SW_FW_SYNC_SW_MAC);
        regs.sw_fw_sync.write(sw_fw_sync);

        // release semaphore
        let swsm = regs.swsm.read() & !(SWSM_SMBI) & !(SWSM_SWESMBI);

        Ok(())
    }