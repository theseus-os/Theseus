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

///device address
const I2C_EEPROM_DEV_ADDR: u8	=0xA0;

/// byte offsets
const SFF_IDENTIFIER: u8 = 0x0
const SFF_1GBE_COMP_CODES: u8 =	0x6;
const SFF_10GBE_COMP_CODES: u8 = 0x3;
const SFF_CABLE_TECHNOLOGY: u8 =	0x8;

/// values at byte offsets
const SFF_IDENTIFIER_SFP: u8 = 	0x3;

//I2C commands
const I2C_READ = u8 = 1;
const I2C_WRITE = u8 = 0;

// bit offsets for Data and Clk in I2CCTL register
const IXGBE_I2C_CLK_IN: u8 = 1 << 0;
const IXGBE_I2C_CLK_OUT: u8 = 1 << 1;
const IXGBE_I2C_DATA_IN: u8 = 1 << 2;
const IXGBE_I2C_DATA_OUT: u8 = 1 << 3;

//wait times in uS for standard I2C operation
/// clock frequency (kHz) in standard I2C mode
const I2C_F_SCL: u8 = 100;
/// Hold time (repeated START condition)
const I2C_T_HD_START: u8 = 4;
/// Low period of the SCL clock
const I2C_T_LOW: u8 = 5; //actually 4.7
/// High period of the SCL clock
const I2C_T_HIGH: u8 = 4;
/// Set up time for a repeated Start condition
const I2C_T_SU_START: u8 = 5; // actually 4.7
/// Data hold time
const I2C_T_HD_DATA: u8 = 5;
/// Data set up time
const I2C_T_SU_DATA: u8 = 1; // actually 250ns
/// Rise time of both SCA and SDA signals
const I2C_T_RISE: u8 = 1; // actually 1000 ns
/// Fall time of both SCA and SDA signals
const I2C_T_FALL:u8 = 1; //actually 300 ns
//set up time for Stop condition
const I2C_T_STU_STOP: u8 = 4;  
// bus free time between a start and stop condition
const I2C_T_BUF:u8 = 5; // actually 4.7

const I2C_CLOCK_STRETCHING_TIMEOUT: u8 = 500;

const SUCCESS: bool = true;
const FAIL: bool = false;





/// Reads 8 bit EEPROM word over I2C interface
/// byte_offset: EEPROM address to read from
fn read_i2c_eeprom(regs: &mut IntelIxgbeRegisters, byte_offset: u8) -> u8 {

    // acquire sw/fw semaphore

    //start of frame
    i2c_start(&mut regs);

    //device address and write indication
    let status = clock_out_i2c_byte(&mut regs, IXGBE_I2C_EEPROM_DEV_ADDR & !(IXGBE_I2C_WRITE));
    let status = get_i2c_ack(&mut regs);

    //byte offset 
    let status = clock_out_i2c_byte(&mut regs, byte_offset);
    let status = get_i2c_ack(&mut regs);
    
    i2c_start(&mut regs);
    
    //device address and read indication
    let status = clock_out_i2c_byte(&mut regs, IXGBE_I2C_EEPROM_DEV_ADDR | IXGBE_I2C_READ);
    let status = get_i2c_ack(&mut regs);

    
    // get data
    let data = clock_in_i2c_byte(&mut regs, byte_offset);
    
    //send NACK
    let status = send_i2c_ack(&mut regs);
    
    //end of frame
    ixgbe_i2c_stop(&mut regs);

    //release semaphore
    data
}

/// switch SDA from high to low, then the SCL from high to loq
fn i2c_start(regs: &mut IntelIxgbeRegisters) {

    //SCL and SDA are both high at the start
    set_i2c_data(regs, true);
    raise_i2c_clk(regs);

    // setup time for start condition (4.7us)
    let _ =pit_clock::pit_wait(I2C_T_SU_START);

    // SDA is shifted to low
    set_i2c_data(regs, false);

    // hold time for start condition (4us)
    let _ =pit_clock::pit_wait(I2C_T_HD_START);

    lower_i2c_clk(regs);

    // minimum low period of the clock (4.7 us)
    let _ =pit_clock::pit_wait(I2C_T_LOW);
}


//switch SCL to high then SDA to high
fn i2c_stop (regs: &mut IntelIxgbeRegisters) {
    //starts with data low and clock high
    set_i2c_data(regs, false);
    raise_i2c_clk(regs);

    // setup time for stop condition (4us)
    let _ =pit_clock::pit_wait(I2C_T_SU_STOP);

    set_i2c_data(regs, true);

    // bus free time between stop and start (4.7 us)
    let _ =pit_clock::pit_wait(I2C_T_BUF);    
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

// change SDA to required bit then change clock from high to low
fn clock_out_i2c_byte (regs: &mut IntelIxgbeRegisters, data: u8) -> Result< (), &'static str> {

    for i in (0..=7).rev() {
        let bit = (data >> i) & 0x1;
        clock_out_i2c_bit(regs, bit)?;
    }

    // release SDA line by setting high
    let i2cctl_val = regs.i2cctl.read() | IXGBE_I2C_DATA_OUT;
    regs.i2cctl.write(i2cctl_val); 

    Ok(())
}

// should receive 1 low bit
fn get_i2c_ack (regs: &mut IntelIxgbeRegisters) -> Result <(), &'static str> {
    let timeout = 10;

    raise_i2c_clk(regs);

    // Minimum high period of clock is 4us
    let _ =pit_clock::pit_wait(I2C_T_HIGH);

    // poll for ACK - a transition from 1 to 0
    for i in 0..timeout {
        let ack = get_i2c_data(regs);
        if !ack {
            break;
        }
    }

    lower_i2c_clk(regs);

    // Minimum low period of clock is 4.7us
    let _ =pit_clock::pit_wait(I2C_T_LOW);

    if ack {
        warn!("Ixgbe phy: ACK not received");
        Err("Ixgbe phy: ACK not received")
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
    raise_i2c_clk(&regs);

    // Minimum high period of clock is 4us
    let _ =pit_clock::pit_wait(I2C_T_HIGH);

    let data = get_i2c_data(regs);

    lower_i2c_clk(&regs);

    // Minimum low period of clock is 4.7us
    let _ =pit_clock::pit_wait(I2C_T_LOW);

    data
}

fn clock_out_i2c_bit(regs: &mut IntelIxgbeRegisters, value: bool) -> Result<(), &'static str> {
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
    let i2cctl_val = 
        if value {
            regs.i22ctl.read() | IXGBE_I2C_DATA_OUT
        }
        else {
            regs.i22ctl.read() & !IXGBE_I2C_DATA_OUT
        };

    regs.i2cctl.write(i2cctl_val);

    // Data rise/fall (1000ns/300ns) and set-up time (250ns)
    let _ =pit_clock::pit_wait(I2C_T_RISE + I2C_T_FALL + I2C_T_SU_DATA);

    // can't check if data was sent correctly if value was 0
    if value == false {
        Ok(())
    }

    //check if data was sent correctly
    if value != get_i2c_data(regs) {
        warn!("Ixgbe phy:: data not sent!");
        Err("Ixgbe phy:: data not sent!")
    }
    
    Ok(())
}

fn get_i2c_data(regs: &mut IntelIxgbeRegisters) -> bool {
    let i2cctl_val = regs.i22ctl.read();

    // data is 1
    if i2cctl_val & IXGBE_I2C_DATA_IN == IXGBE_I2C_DATA_IN {
        true
    }
    // data is 0
    else {
        false
    }
}

fn raise_i2c_clk(regs: &mut IntelIxgbeRegisters) {
    let timeout = I2C_CLOCK_STRETCHING_TIMEOUT;

    for i in 0..timeout{
        let val = regs.i2cctl.read();
        regs.i2cctl.write(val | IXGBE_I2C_CLK_OUT));

        // SCL rise time (1000ns)
        let _ =pit_clock::pit_wait(I2C_T_RISE);

        let val = regs.i2cctl.read();
        if (val & IXGBE_I2C_CLK_IN) == IXGBE_I2C_CLK_IN {
            break;
        }
    }
}

fn lower_i2c_clk(regs: &mut IntelIxgbeRegisters) {

    let val = regs.i2cctl.read();
    regs.i2cctl.write(val & !IXGBE_I2C_CLK_OUT);

    // SCL fall time (300ns)
    let _ =pit_clock::pit_wait(I2C_T_FALL);
}