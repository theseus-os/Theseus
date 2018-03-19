
pub const PILOT_LENGTH_BYTES:       usize = 2760;
pub const DATA_LENGTH_BYTES:        usize = 7704;
pub const MAX_ETH_PAYLOAD:          usize = 1500;
pub const PILOT_LENGTH_IQ:          usize = 684;// (2760 - 22 (header) - 2 (end padding)) /4 
pub const DATA_LENGTH_IQ:           usize = 684;
pub const ARGOS_PACKET_HEADER:      usize = 22;

pub struct PilotPacketBytes {
   pub buffer: [u8;PILOT_LENGTH_BYTES],        
}

pub struct DataPacketBytes {
    buffer: [u8;DATA_LENGTH_BYTES],        
}

pub struct PilotPacketIQ {
   pub buffer: [f32;PILOT_LENGTH_IQ*2], //multiply by two b/c Re and Im parts stored separately        
}

pub struct DataPacketIQ {
    buffer: [f32;DATA_LENGTH_IQ*2], //multiply by two b/c Re and Im parts stored separately         
}