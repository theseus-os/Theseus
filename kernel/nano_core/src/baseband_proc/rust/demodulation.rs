const MAXLLR:			f64 = 200;
const MINLLR:			f64 = -200;

pub fn demodulation(mask: [i32], input: [f64], output: [i16]){
	let mut outptr: u32 = 0;
	for i in 0..64 {
		if (mask[i] == 1) && (i != 7) && (i != 21) && (i != 43)) && (i != 57){
			// 16QAM  4 bits per symble
			let Noise: f64 = 0.5;
			let rI: f64 = input[i * 2];
			let rQ: f64 = input[i * 2 + 1];

            //maximum likelihood estimation
			let mut b1: f64 = -log((exp(-(rI - 1)*(rI - 1) / Noise) + exp(-(rI - 3)*(rI - 3) / Noise)) /
				(exp(-(rI + 1)*(rI + 1) / Noise) + exp(-(rI + 3)*(rI + 3) / Noise)));

			let mut b3: f64 = -log((exp(-(rI - 1)*(rI - 1) / Noise) + exp(-(rI + 1)*(rI + 1) / Noise)) /
				(exp(-(rI - 3)*(rI - 3) / Noise) + exp(-(rI + 3)*(rI + 3) / Noise)));

			let mut b2: f64 = -log((exp(-(rQ - 1)*(rQ - 1) / Noise) + exp(-(rQ - 3)*(rQ - 3) / Noise)) /
				(exp(-(rQ + 1)*(rQ + 1) / Noise) + exp(-(rQ + 3)*(rQ + 3) / Noise)));

			let mut b4: f64 = -log((exp(-(rQ - 1)*(rQ - 1) / Noise) + exp(-(rQ + 1)*(rQ + 1) / Noise)) /
				(exp(-(rQ - 3)*(rQ - 3) / Noise) + exp(-(rQ + 3)*(rQ + 3) / Noise)));

			//printf("(%d) b1 b2 b3 b4 [%.2f %.2f %.2f %.2f ]\n", outptr, b1, b2, b3, b4);

			b1 *= 2.0;
			b2 *= 2.0;
			b3 *= 2.0;
			b4 *= 2.0;


			if (b1 > MAXLLR) b1 = MAXLLR;
			if (b1 < MINLLR) b1 = MINLLR;

			if (b2 > MAXLLR) b2 = MAXLLR;
			if (b2 < MINLLR) b2 = MINLLR;

			if (b3 > MAXLLR) b3 = MAXLLR;
			if (b3 < MINLLR) b3 = MINLLR;

			if (b4 > MAXLLR) b4 = MAXLLR;
			if (b4 < MINLLR) b4 = MINLLR;

			output[outptr] = floor(b1);
			outptr++;
			output[outptr] = floor(b2);
			outptr++;
			output[outptr] = floor(b3);
			outptr++;
			output[outptr] = floor(b4);
			outptr++;

		}
	}

}