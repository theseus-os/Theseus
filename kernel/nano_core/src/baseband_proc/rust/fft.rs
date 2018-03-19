const DFT_LENGTH:			usize = 64;

// (I - float, Q - float) <-- complex representation    length = 64
/// implementing basi formula X[k] = summation _ 0 to N (x[n] * e^(-i*2*pi*k*n)/N) 
fn DFT(input: &[f64], output: &mut [f64])
{
	//TODO DFT ~~~ https://jakevdp.github.io/blog/2013/08/28/understanding-the-fft/
	pi: f64 = 3.1415926;

	for k in 0..DFT_LENGTH
	{
		output[k * 2] = 0;
		output[k * 2 + 1] = 0;
		for (int n = 0; n < 64; n++)
		{
			let cosvalue: f64 = (-2 * pi*k*n / 64).cos();
			let sinvalue: f64 = (-2 * pi*k*n / 64).sin();
			let a: f64 = input[n * 2];
			let b: f64 = input[n * 2 + 1];

			output[k * 2] += a*cosvalue - b*sinvalue;
			output[k * 2 + 1] += a*sinvalue + b*cosvalue;

		}

	}

}