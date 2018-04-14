/// LTS: Long Training Sequence

use irq_safety::MutexIrqSafe;
use baseband_proc::packet_types::*;
use baseband_proc::fourier_transform::*;

pub const LTS_LENGTH:			    usize = 160;
const M_XCORR:			            usize = 1536/2;

pub static LTS_ORIG : MutexIrqSafe<[Complex; LTS_LENGTH]> = MutexIrqSafe::new([Complex{real: 0.0, imag: 0.0};LTS_LENGTH]);
pub static M_LTS : MutexIrqSafe<[Complex; NUM_SUBCARRIERS]> = MutexIrqSafe::new( [Complex{real: 0.0, imag: 0.0}; NUM_SUBCARRIERS] );
//mask tells which subcarriers are used for data and which as pilots?
pub static M_MASK : MutexIrqSafe<[i32;NUM_SUBCARRIERS]> = MutexIrqSafe::new([0; NUM_SUBCARRIERS]);

///load LTS from file as f32 numbers and then store in a Complex array and apply fourier transform
/// LTS.csv
/// Now just loading as an array
pub fn load_lts() {

    let lts_raw : [f32; LTS_LENGTH*2] = [-0.972566,0.076464549,0.570883055,-0.571947471,-0.017465397,0.467290821,-0.79252131,-0.758676911,-0.218111804,-0.351400882,-0.375395876,0.432951588,0.511762027,-0.817034657,-0.35607678,0.229793072,0.389026749,0.74219512,-0.139945099,0.365179516,0.152348175,-0.851532104,0.006155833,0.331996887,0.607138553,-0.23849498,-0.716625956,0.372369214,0.131408694,0.602723093,0.247419135,-0.031876854,0.972566873,-0.031876854,0.247419135,0.602723093,0.131408694,0.372369214,-0.716625956,-0.23849498,0.607138553,0.331996887,0.006155833,-0.851532104,0.152348175,0.365179516,-0.139945099,0.74219512,0.389026749,0.229793072,-0.35607678,-0.817034657,0.511762027,0.432951588,-0.375395876,-0.351400882,-0.218111804,-0.758676911,-0.79252131,0.467290821,-0.017465397,-0.571947471,0.570883055,0.076464549,-0.972566873,0.076464549,0.570883055,-0.571947471,-0.017465397,0.467290821,-0.79252131,-0.758676911,-0.218111804,-0.351400882,-0.375395876,0.432951588,0.511762027,-0.817034657,-0.35607678,0.229793072,0.389026749,0.74219512,-0.139945099,0.365179516,0.152348175,-0.851532104,0.006155833,0.331996887,0.607138553,-0.23849498,-0.716625956,0.372369214,0.131408694,0.602723093,0.247419135,-0.031876854,0.972566873,-0.031876854,0.247419135,0.602723093,0.131408694,0.372369214,-0.716625956,-0.23849498,0.607138553,0.331996887,0.006155833,-0.851532104,0.152348175,0.365179516,-0.139945099,0.74219512,0.389026749,0.229793072,-0.35607678,-0.817034657,0.511762027,0.432951588,-0.375395876,-0.351400882,-0.218111804,-0.758676911,-0.79252131,0.467290821,-0.017465397,-0.571947471,0.570883055,0.076464549,-0.972566873,0.076464549,0.570883055,-0.571947471,-0.017465397,0.467290821,-0.79252131,-0.758676911,-0.218111804,-0.351400882,-0.375395876,0.432951588,0.511762027,-0.817034657,-0.35607678,0.229793072,0.389026749,0.74219512,-0.139945099,0.365179516,0.152348175,-0.851532104,0.006155833,0.331996887,0.607138553,-0.23849498,-0.716625956,0.372369214,0.131408694,0.602723093,0.247419135,-0.031876854,0.0,-0.607501393,-0.658990523,-0.716610358,-0.334714049,0.460859256,0.127609363,0.103115232,0.939193654,0.135716934,-0.505959626,-0.087901114,-0.574866707,-0.40600219,-0.244611232,-0.612136082,0.389026749,0.025492733,-1.0,0.092986726,0.364326947,0.294912224,0.715838123,-0.025372801,0.161140156,0.6608532,0.34346702,0.545924411,-0.173573894,-0.515369625,0.691894612,0.748955124,0.0,-0.748955124,-0.691894612,0.515369625,0.173573894,-0.545924411,-0.34346702,-0.6608532,-0.161140156,0.025372801,-0.715838123,-0.294912224,-0.364326947,-0.092986726,1.0,-0.025492733,-0.389026749,0.612136082,0.244611232,0.40600219,0.574866707,0.087901114,0.505959626,-0.135716934,-0.939193654,-0.103115232,-0.127609363,-0.460859256,0.334714049,0.716610358,0.658990523,0.607501393,0.0,-0.607501393,-0.658990523,-0.716610358,-0.334714049,0.460859256,0.127609363,0.103115232,0.939193654,0.135716934,-0.505959626,-0.087901114,-0.574866707,-0.40600219,-0.244611232,-0.612136082,0.389026749,0.025492733,-1.0,0.092986726,0.364326947,0.294912224,0.715838123,-0.025372801,0.161140156,0.6608532,0.34346702,0.545924411,-0.173573894,-0.515369625,0.691894612,0.748955124,0.0,-0.748955124,-0.691894612,0.515369625,0.173573894,-0.545924411,-0.34346702,-0.6608532,-0.161140156,0.025372801,-0.715838123,-0.294912224,-0.364326947,-0.092986726,1.0,-0.025492733,-0.389026749,0.612136082,0.244611232,0.40600219,0.574866707,0.087901114,0.505959626,-0.135716934,-0.939193654,-0.103115232,-0.127609363,-0.460859256,0.334714049,0.716610358,0.658990523,0.607501393,0.0,-0.607501393,-0.658990523,-0.716610358,-0.334714049,0.460859256,0.127609363,0.103115232,0.939193654,0.135716934,-0.505959626,-0.087901114,-0.574866707,-0.40600219,-0.244611232,-0.612136082,0.389026749,0.025492733,-1.0,0.092986726,0.364326947,0.294912224,0.715838123,-0.025372801,0.161140156,0.6608532,0.34346702,0.545924411,-0.173573894,-0.515369625,0.691894612,0.748955124];
    let mut lts_complex : [Complex; LTS_LENGTH] = [Complex{real: 0.0, imag: 0.0}; LTS_LENGTH];

    // store LTS as Complex numbers
    for i in 0..LTS_LENGTH {
        lts_complex[i].real = lts_raw[i];
    }

    for i in 160..LTS_LENGTH*2 {
        lts_complex[i-160].imag = lts_raw[i];
    }

    //store in LTS global variable
    let mut lts_orig = LTS_ORIG.lock();

    for i in 0..LTS_LENGTH {
        lts_orig[i] = lts_complex[i];
    }

    //take the fourier transform
    let res = dft(&lts_complex,64); // TODO: why the 64 offset?

    //store in global variable
    let mut lts = M_LTS.lock();

    for i in 0..NUM_SUBCARRIERS {
        lts[i] = res[i];
    }

    //store the mask subcarriers
    let mut mask = M_MASK.lock();

    for i in 0..NUM_SUBCARRIERS {

		//if absolute value of lts is > 0.01 then store a 1
		if (lts[i].real > 0.01 || lts[i].real < -0.01) || (lts[i].imag > 0.01 || lts[i].imag < -0.01)
		{
			mask[i] = 1;
		}
	}


}

/// Do the correlation to find the LTS, set up the start position of LTS Symbol and Data Symbols
/// length -- This parameter is not the length of input, instead it is the length for the correlation.
fn xcorr(length: i32, data: DataPacketIQ) -> i32 {
    let mut m_xcorr: [Complex; M_XCORR] = [Complex{real: 0.0, imag: 0.0}; M_XCORR];	// The length should not exceed 256.
	if length > 384
	{
		debug!("Error! Increase the size of m_xcorr.\n");
		return 0;
	}

    let lts_orig = LTS_ORIG.lock();

	for i in 1..(length * 2)
	{
		let n: i32 = (i - length) as i32;

		if n < 0 {
			let mut m: i32 = -n; 
			while  (m + n < 160) && (m < length)
			{
				// (a-bi) * (c+di)
				// = ac + bd + i*(ad - bc)
				let x = data.buffer[m as usize];
				let l = lts_orig[(m + n) as usize];

				m_xcorr[i as usize] +=  x*l;
				
				m = m+1;

			}
		}
		else
		{
			for m in 0..(160 - n)
			{
				// (a-bi) * (c+di)
				// = ac + bd + i*(ad - bc)
				let x = data.buffer[m as usize];
				let l = lts_orig[(m + n) as usize];

				m_xcorr[i as usize] +=  x*l;

			}
		}


	}

	let mut peak: i32 = 0;
	let mut maxkey: f64 = 0.0;


	for i in (length-64)..(length +64)
	{
		if ((m_xcorr[i as usize].real * m_xcorr[i as usize].real) + (m_xcorr[i as usize].imag * m_xcorr[i as usize].imag)) as f64 > maxkey
		{
			maxkey = ((m_xcorr[i as usize].real * m_xcorr[i as usize].real) + (m_xcorr[i as usize].imag * m_xcorr[i as usize].imag)) as f64;
			peak = i;
		}
	}

	return length - peak;
} 