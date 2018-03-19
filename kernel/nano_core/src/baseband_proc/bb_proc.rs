use irq_safety::MutexIrqSafe;
use alloc::Vec;
use baseband_proc::fifo::FIFO;
use baseband_proc::packet_types::{PilotPacketBytes, PilotPacketIQ, PILOT_LENGTH_IQ,ARGOS_PACKET_HEADER};

lazy_static! {
    pub static ref FIFO_RAW_PILOTS: MutexIrqSafe<FIFO<PilotPacketBytes>> = MutexIrqSafe::new(FIFO { buffer: Vec::new() } );
}

pub fn process_data() {
    let mut fifo = FIFO_RAW_PILOTS.lock();

    if fifo.is_empty() {
        debug!("FIFO is empty!");
    }
    else {
        debug!("processing data!");
        let pkt_b = fifo.pop();
        /* for i in 0..20 {
            debug!("buffer:{:x}", a.buffer[i]);
        }        
        debug!("buffer:{:x}", a.buffer[1493]); */

        let antenna_id = pkt_b.buffer[21]-1;

		let frame_id = pkt_b.buffer[17] + pkt_b.buffer[16]*256 + pkt_b.buffer[15]*256*256 + pkt_b.buffer[14]*256*256*256;

        //convert from bytes to complex number
        let pkt_iq = bytes_to_complex(pkt_b)

        //apply cross correlation

        //apply DFT

        //store in array for beamforming
    }
}

fn bytes_to_IQ(input: PilotPacketBytes) -> PilotPacketIQ {

    let mut output = PilotPacketIQ {buffer: [0;PILOT_LENGTH_IQ*2]};

    for i in 0..PILOT_LENGTH_IQ {
		
        let mut I,Q :f32;

		Q = input[ARGOS_PACKET_HEADER + i * 4 + 3];
		Q = (Q << 8) | input[ARGOS_PACKET_HEADER + i * 4 + 2];

		I = input[ARGOS_PACKET_HEADER + i * 4 + 1];
		I = (I << 8) | input[ARGOS_PACKET_HEADER + i * 4 + 0];

		output[i * 2] = I / 32768.0; //?
		output[i * 2 + 1] = Q / 32768.0; //?

	}
    return output;
}

fn xcorr() {

}

fn dft() {

}

