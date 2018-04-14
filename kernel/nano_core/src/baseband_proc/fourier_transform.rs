use baseband_proc::packet_types::*;
use baseband_proc::trig::{cos,sin};
use alloc::Vec;

// (I - float, Q - float) <-- complex representation    length = 64
// implementing basi formula X[k] = summation _ 0 to N (x[n] * e^(-i*2*pi*k*n)/N) 
pub fn dft(input: &[Complex], offset: usize) -> [Complex;NUM_SUBCARRIERS] {
    let mut output = [Complex{real: 0.0, imag: 0.0};NUM_SUBCARRIERS]; 
    let pi: f32 = 3.1415926;

	for k in 0..DFT_LENGTH
	{
		output[k * 2].real = 0.0;
		output[k * 2].imag = 0.0;
		for n in 0..DFT_LENGTH
		{
			let cosvalue: f32 = cos(-2.0 * pi * k as f32 *n as f32 / 64.0);
			let sinvalue: f32 = sin(-2.0 * pi * k as f32 *n as f32 / 64.0);
			let a: f32 = input[(offset*2) + n * 2].real;
			let b: f32 = input[(offset*2) + n * 2].imag;

			output[k * 2].real += a*cosvalue - b*sinvalue;
			output[k * 2].imag += a*sinvalue + b*cosvalue;

		}

	}

    return output;
}

// https://rosettacode.org/wiki/Fast_Fourier_transform#C.2B.2B

pub fn fft(input: &mut Vec<Complex>) {
    //let mut output = Symbol{data: [Complex{real: 0.0, imag: 0.0};NUM_SUBCARRIERS] }; 
    let pi: f32 = 3.1415926;

	let N = input.len();
	if N <= 1 {
		return;
	}

	//divide
	let mut even : Vec<Complex> = Vec::with_capacity(N/2);
	let mut odd : Vec<Complex> = Vec::with_capacity(N/2);
	
	divide_array(0, N/2, 2, input, &mut even);
	divide_array(1, N/2, 2, input, &mut odd);

	fft(&mut even);
	fft(&mut odd);

	//combine
	let mut k = 0;
	while k < N/2 {
		k += 1;
		let t = Complex{ real: cos(-2.0*pi*k as f32/  N as f32), imag: sin(-2.0*pi*k as f32 / N as f32)} * odd[k];
		input[k] = even[k] + t;
		input[k + N/2] = even[k] -t;
	}
}

fn divide_array(start: usize, size: usize, stride: usize, input: &Vec<Complex>, output: &mut Vec<Complex>) {
	
	for i in 0..size {
		output.push(input[start + (i*stride)]);	
	}

}