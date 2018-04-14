use baseband_proc::packet_types::*;
use baseband_proc::lts::*;
use baseband_proc::fourier_transform::*;


pub fn process_pilot() {
    let mut fifo = FIFO_RAW_PILOTS.lock();

    if fifo.is_empty() {
        debug!("FIFO is empty!");
    }
    else {
        debug!("processing pilot!");
        let pkt_b = fifo.pop();
        //fifo.drop();
      
        //convert from bytes to complex number
        //let pkt_iq = bytes_to_IQ(pkt_b);
        //store package in fifo
        
        //apply cross correlation

        //apply DFT

        //store in CSI array
    }
}

pub fn process_data() {
    let mut fifo = FIFO_RAW_DATA.lock();

    if fifo.is_empty() {
        debug!("FIFO is empty!");
    }
    else {
        debug!("processing data!");
        let pkt_b = fifo.pop();
        //fifo.drop();
      
        //convert from bytes to complex number
        //let pkt_iq = bytes_to_IQ(pkt_b);
        //store package in fifo
        
        //apply cross correlation

        //apply DFT

        //apply zero-forcing
    }
}

 fn bytes_to_iq(input: PilotPacketBytes) -> PilotPacketIQ {

    let mut output = PilotPacketIQ {
                antenna_id: (input.buffer[21] as i32) -1,
                frame_id: (input.buffer[17] as i32) + (input.buffer[16] as i32)*256 + (input.buffer[15] as i32)*256*256 + (input.buffer[14] as i32)*256*256*256,
                buffer: [Complex{real: 0.0, imag: 0.0}; PILOT_LENGTH_IQ]};

    for i in 0..PILOT_LENGTH_IQ {
		
        let mut I:f32;
        let mut Q:f32;

		Q = input.buffer[(ARGOS_PACKET_HEADER as usize) + (i as usize) * 4 + 3] as f32;
		Q = (Q * 256.0) + input.buffer[ARGOS_PACKET_HEADER + i * 4 + 2] as f32;

		I = input.buffer[ARGOS_PACKET_HEADER as usize + i as usize * 4 + 1] as f32;
		I = (I * 256.0) + input.buffer[ARGOS_PACKET_HEADER + i * 4 + 0] as f32;

		output.buffer[i * 2].real = I / 32768.0; //TODO:?
		output.buffer[i * 2].imag = Q / 32768.0; //TODO:?

	}
    return output;
}


  

// assume you have a channel matrix w/ dimensions NO OF ANTENNA x NO OF USERS 
// There is a channel matrix for each sub carrier
pub fn find_bf_weights(subcarrier_no : usize) {

    /**** find bf weights to be used on the transmit part ****/
    let mut h_transpose : [[Complex; NUM_ANTENNAS]; NUM_USERS] = [[Complex{real: 0.0, imag: 0.0}; NUM_ANTENNAS]; NUM_USERS];
    let mut h_conj : [[Complex; NUM_USERS]; NUM_ANTENNAS] = [[Complex{real: 0.0, imag: 0.0}; NUM_USERS]; NUM_ANTENNAS];
    let mut h_mul : [[Complex;NUM_USERS]; NUM_USERS] = [[Complex{real: 0.0, imag: 0.0}; NUM_USERS]; NUM_USERS];
    let mut h_inv : [[Complex;NUM_USERS]; NUM_USERS] = [[Complex{real: 0.0, imag: 0.0}; NUM_USERS]; NUM_USERS];

    let pilot_fifo = CSI.lock();

    // find transpose (H^T)
    for i in 0..NUM_ANTENNAS {
        for j in 0..NUM_USERS {
            h_transpose[j][i] = pilot_fifo[i][j].LTS[subcarrier_no];
        }
    }

    //find conjugate (H*)
    for i in 0..NUM_ANTENNAS {
        for j in 0..NUM_USERS {
            h_conj[i][j] = pilot_fifo[i][j].LTS[subcarrier_no].conj();
        }
    }

    //multiply transpose and conjugate (H^T)(H*)
    for i in 0..NUM_USERS {
        for j in 0..NUM_USERS{
            for k in 0..NUM_ANTENNAS{
                h_mul[i][j] += h_transpose[i][k] * h_conj[k][j]; 
            }
        }
    }

    //find inverse ((H^T)(H*))^-1 .... just implemented for a 2x2 matrix
    //TODO: will need to change to a more generalized version when there are more than 2 users
    let mut determinant = Complex{real: 1.0, imag: 0.0};
    // 1/(ad-bc)
    determinant = determinant / ( (h_mul[0][0] * h_mul[1][1]) - (h_mul[0][1] * h_mul[1][0]) );
    
    h_inv[0][0] = h_mul[1][1] / determinant;

    h_inv[0][1] = -h_mul[0][1] / determinant;
    
    h_inv[1][0] = -h_mul[1][0] / determinant;

    h_inv[1][1] = h_mul[0][0] / determinant;


    //multiply conjugate and inverse (H*)(((H^T)(H*))^-1) and store in beamforming weights

    let mut bf_weights_t = BF_WEIGHTS_TRANSMIT.lock();


    for i in 0..NUM_ANTENNAS {
        for j in 0..NUM_USERS{
            for k in 0..NUM_USERS{
                bf_weights_t[subcarrier_no][i][j] +=  h_conj[i][k] * h_inv[k][j]; 
            }
        }
    }

    /**** find left inverse of bf weights to be used on the receive part ****/
    //((A^T*A)^-1)*A^T

    let bf_weights_r = BF_WEIGHTS_RECEIVE.lock();
    //multiply (A^T*A) -> results in a square matrix

    //Take inverse of the square matrix

    //multiply inverse with A^T

}



//apply bf weights to each sub-carrier and store in transmission Vector
//right now assume only one data symbol with 64 subcarriers
pub fn zero_forcing_recive(subcarrier_no : usize, user_no : usize) {
    let bf_weights = BF_WEIGHTS_RECEIVE.lock();
    let t_vec = RECEIVE_VECTOR.lock();
    let data = DATA_OFDM.lock();

    for i in 0..NUM_ANTENNAS {
        //t_vec[subcarrier_no][i][0] += bf_weights[i][k].mul(h_inv[k][j]); 
    }

} 


