use irq_safety::MutexIrqSafe;
use alloc::Vec;
use baseband_proc::fifo::FIFO;
use core::ops::{Add, Sub, Mul, Div, AddAssign, Neg};

pub const PILOT_LENGTH_BYTES:       usize = 8214;
pub const DATA_LENGTH_BYTES:        usize = 8214;
pub const MAX_ETH_PAYLOAD:          usize = 1500;
pub const PILOT_LENGTH_IQ:          usize = 2048;
pub const DATA_LENGTH_IQ:           usize = 2048;
pub const ARGOS_PACKET_HEADER:      usize = 22;
pub const NUM_USERS:                usize = 2;
pub const NUM_ANTENNAS:             usize = 88;
pub const DATA_SYMBOL_NUM:          usize = 20;
pub const NUM_SUBCARRIERS:          usize = 64;
pub const DFT_LENGTH:			    usize = 64;

#[derive(Copy, Clone)]
pub struct Complex {
    pub real: f32,
    pub imag: f32
}

impl Add for Complex {
    type Output = Complex;

    fn add(self, other: Complex) -> Complex {
        Complex{ 
            real: self.real + other.real, 
            imag: self.imag + other.imag
        }
    }
}

impl Sub for Complex {
    type Output = Complex;

    fn sub(self, other: Complex) -> Complex {
        Complex { 
            real: self.real - other.real, 
            imag: self.imag - other.imag
        }
    }
}

impl Mul for Complex {
    type Output = Complex;

    fn mul(self, other: Complex) -> Complex {
        Complex { real: (self.real * other.real) - (self.imag * other.imag), 
                  imag: (self.real * other.imag) + (self.imag * other.real)
        }
    }
}

impl Div for Complex {
    type Output = Complex;

    fn div(self, other: Complex) -> Complex {
        let denom = other * other.conj();
        let num = self * other.conj();
        Complex { 
                  real: num.real/denom.real, 
                  imag: num.imag/denom.real
        }
    }
}

impl AddAssign for Complex {
    fn add_assign(&mut self, other: Complex) {
        *self = Complex{ 
            real: self.real + other.real, 
            imag: self.imag + other.imag
        }
    }
}

impl Neg for Complex {
    type Output = Complex;

    fn neg(self) -> Complex {
        Complex { 
                  real: -self.real, 
                  imag: -self.imag
        }
    }

}
impl Complex {

    pub fn conj(&self) -> Complex{
        return Complex{ real: self.real, imag: -self.imag}
    }

    pub fn add(&self, arg2: Complex) -> Complex {
        return Complex{ real: self.real + arg2.real, imag: self.imag + arg2.imag}
    }
    
    pub fn sub(&self, arg2: Complex) -> Complex {
        return Complex{ real: self.real - arg2.real, imag: self.imag - arg2.imag}
    }
    
    pub fn mul(&self, arg2: Complex) -> Complex {
        return Complex{ real: (self.real * arg2.real) - (self.imag * arg2.imag), 
                        imag: (self.real * arg2.imag) + (self.imag * arg2.real)
                        }
    }

    pub fn div(&self, arg2: Complex) -> Complex {
        let denom = arg2.mul(arg2.conj());
        let num = self.mul(arg2.conj());
        return Complex{ real: num.real/denom.real, 
                        imag: num.imag/denom.real
                        }
    }

    pub fn div_by_scalar(&self, arg2: f32) -> Complex {
        return Complex{ real: self.real/arg2, 
                        imag: self.imag/arg2
                        }
    }

    pub fn negate(&mut self) {
        self.real = -self.real;
        self.imag = -self.imag;
    }
}

pub struct PilotPacketBytes {
   pub buffer: [u8;PILOT_LENGTH_BYTES],        
}

pub struct DataPacketBytes {
    pub buffer: [u8;DATA_LENGTH_BYTES],        
}

pub struct PilotPacketIQ {
    pub antenna_id: i32,
    pub frame_id: i32,
    pub buffer: [Complex;PILOT_LENGTH_IQ],       
}

pub struct DataPacketIQ {
    pub antenna_id: i32,
    pub frame_id: i32,
    pub buffer: [Complex;DATA_LENGTH_IQ],        
}

#[derive(Copy, Clone)]
pub struct Symbol {
	pub data:   [Complex;NUM_SUBCARRIERS]	
}

#[derive(Copy, Clone)]
pub struct Pilot {
    pub LTS:    [Complex;NUM_SUBCARRIERS]
}

#[derive(Copy, Clone)]
pub struct Data {
    pub LTS:    [Complex;NUM_SUBCARRIERS],
    pub OFDM_data: [[Complex;NUM_SUBCARRIERS];DATA_SYMBOL_NUM]    
}

/****** PILOTS ******/

// where pilot packets are stored once they're received from the nic
lazy_static! {
    pub static ref FIFO_RAW_PILOTS: MutexIrqSafe<FIFO<PilotPacketBytes>> = MutexIrqSafe::new(FIFO { buffer: Vec::new() } );
}

//where pilot packets are stored once they're converted from bytes to IQ samples
lazy_static! {
    pub static ref FIFO_PILOTS: MutexIrqSafe<FIFO<PilotPacketIQ>> = MutexIrqSafe::new(FIFO { buffer: Vec::new() } );
}

//where the CSI of the pilots are stored (after applying the fft)
//each Pilot CSI is made of NUM_SUBCARRIERS
pub static CSI: MutexIrqSafe<[[Pilot; NUM_USERS]; NUM_ANTENNAS]> = MutexIrqSafe::new([[Pilot {LTS: [Complex{real: 0.0, imag: 0.0}; NUM_SUBCARRIERS] }; NUM_USERS]; NUM_ANTENNAS]);



/****** DATA ******/

// where data packets are stored once they're received from the nic
lazy_static! {
    pub static ref FIFO_RAW_DATA: MutexIrqSafe<FIFO<DataPacketBytes>> = MutexIrqSafe::new(FIFO { buffer: Vec::new() } );
}

//where data packets are stored once they're converted from bytes to IQ samples
lazy_static! {
    pub static ref FIFO_DATA: MutexIrqSafe<FIFO<DataPacketIQ>> = MutexIrqSafe::new(FIFO { buffer: Vec::new() } );
}

//where the OFDM symbols of data is stored
//each data symbol is made up of NUM_SUBCARRIERS
pub static DATA_OFDM: MutexIrqSafe<[[Data; NUM_USERS]; NUM_ANTENNAS]> = MutexIrqSafe::new([[Data {LTS: [Complex{real: 0.0, imag: 0.0};NUM_SUBCARRIERS], OFDM_data: [[Complex{real: 0.0, imag: 0.0};NUM_SUBCARRIERS];DATA_SYMBOL_NUM] }; NUM_USERS]; NUM_ANTENNAS]);



/******* BEAMFORMING WEIGHT MATRIX AND ZEROFORCING ********/

//where the beamforming matrix is stored after being calculated from the CSI
//the beamforming matrix for each sub carrier is stored in its own NUM_ANTENNAS x NUM_USERS array 
//this is done so we can apply the zero-forcing to one set of subcarriers while the other weights are still being calculated
pub static BF_WEIGHTS_TRANSMIT: MutexIrqSafe<[[[Complex; NUM_USERS]; NUM_ANTENNAS]; NUM_SUBCARRIERS]> = MutexIrqSafe::new([[[Complex{real: 0.0, imag: 0.0}; NUM_USERS]; NUM_ANTENNAS]; NUM_SUBCARRIERS]);

//The inverse of the previous beamforming weights
pub static BF_WEIGHTS_RECEIVE: MutexIrqSafe<[[[Complex; NUM_ANTENNAS]; NUM_USERS]; NUM_SUBCARRIERS]> = MutexIrqSafe::new([[[Complex{real: 0.0, imag: 0.0}; NUM_ANTENNAS]; NUM_USERS]; NUM_SUBCARRIERS]);

//the final data to be sent is store here
pub static RECEIVE_VECTOR: MutexIrqSafe<[[[Complex; 1]; NUM_ANTENNAS]; NUM_SUBCARRIERS]> = MutexIrqSafe::new([[[Complex{real: 0.0, imag: 0.0}; 1]; NUM_ANTENNAS];NUM_SUBCARRIERS]);
