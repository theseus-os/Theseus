
pub fn equalization(input: [f64], output: [f64], eqCSI: [f64], mask: [i32])
{

	for i = 0..64
	{
		output[i * 2] = 0;
		output[i * 2 + 1] = 0;

		if mask[i] == 1
		{
			//printf("Mask %d\n", i);
			// (a+bi) / (c+di)
			let a: f64 = input[i * 2];
			let b: f64 = input[i * 2 + 1];
			let c: f64 = eqCSI[i * 2];
			let d: f64 = eqCSI[i * 2 + 1];

			output[i * 2] = (a*c + b*d) / (c*c + d*d);
			output[i * 2 + 1] = (c*b - a*d) / (c*c + d*d);
		}
	}


	//Correct the phase

	let M1[f64;2] = { 1, 0 };
	let M2[f64;2] = { 0, 1 };
	let M3[f64;2] = { -1, 0 };
	let M4[f64;2] = { 0, -1 };

	let T1_real: f64, T1_imag: f64;
	let T2_real: f64, T2_imag: f64;
	let T3_real: f64, T3_imag: f64;
	let T4_real: f64, T4_imag: f64;

	let PhaseCorrection[f64;2];

	let P1[f64;2], P2[f64;2], P3[f64;2], P4[f64;2];

	P1[0] = output[7 * 2] / sqrtf(output[7 * 2] * output[7 * 2] + output[7 * 2 + 1] * output[7 * 2 + 1]);
	P1[1] = output[7 * 2 + 1] / sqrtf(output[7 * 2] * output[7 * 2] + output[7 * 2 + 1] * output[7 * 2 + 1]);
	let A1: f64 = sqrtf(output[7 * 2] * output[7 * 2] + output[7 * 2 + 1] * output[7 * 2 + 1]);


	P2[0] = output[21 * 2] / sqrtf(output[21 * 2] * output[21 * 2] + output[21 * 2 + 1] * output[21 * 2 + 1]);
	P2[1] = output[21 * 2 + 1] / sqrtf(output[21 * 2] * output[21 * 2] + output[21 * 2 + 1] * output[21 * 2 + 1]);
	let A2: f64 = sqrtf(output[21 * 2] * output[21 * 2] + output[21 * 2 + 1] * output[21 * 2 + 1]);


	P3[0] = output[43 * 2] / sqrtf(output[43 * 2] * output[43 * 2] + output[43 * 2 + 1] * output[43 * 2 + 1]);
	P3[1] = output[43 * 2 + 1] / sqrtf(output[43 * 2] * output[43 * 2] + output[43 * 2 + 1] * output[43 * 2 + 1]);
	let A3: f64 = sqrtf(output[43 * 2] * output[43 * 2] + output[43 * 2 + 1] * output[43 * 2 + 1]);

	P4[0] = output[57 * 2] / sqrtf(output[57 * 2] * output[57 * 2] + output[57 * 2 + 1] * output[57 * 2 + 1]);
	P4[1] = output[57 * 2 + 1] / sqrtf(output[57 * 2] * output[57 * 2] + output[57 * 2 + 1] * output[57 * 2 + 1]);
	let A4: f64 = sqrtf(output[57 * 2] * output[57 * 2] + output[57 * 2 + 1] * output[57 * 2 + 1]);


	complexDivision(M1, P1, &T1_real, &T1_imag);
	complexDivision(M2, P2, &T2_real, &T2_imag);
	complexDivision(M3, P3, &T3_real, &T3_imag);
	complexDivision(M4, P4, &T4_real, &T4_imag);



	PhaseCorrection[0] = 0.25*(T1_real + T2_real + T3_real + T4_real);
	PhaseCorrection[1] = 0.25*(T1_imag + T2_imag + T3_imag + T4_imag);

	let AA: f64 = 4.2426 / (0.25*(A1 + A2 + A3 + A4));

	
	for i in 0..64
	{
		if mask[i] == 1
		{
        	complexMultiply(&(output[i * 2]), PhaseCorrection, &(output[i * 2]), &(output[i * 2 + 1]));
			output[i * 2] *= AA;
			output[i * 2 + 1] *= AA;

		}
	}

}