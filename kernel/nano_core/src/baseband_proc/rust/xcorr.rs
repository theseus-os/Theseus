/// Do the correlation to find the LTS, set up the start position of LTS Symbol and Data Symbols
/// length -- This parameter is not the length of input, instead it is the length for the correlation.

const M_XCORR:			usize = 1536;

fn findLTS(length: i32, mIQRawData: [f64], mOriginalLTS: [f64]) -> i32{
	//TODO Correlation ~~~ https://en.wikipedia.org/wiki/Cross-correlation
	  
	let mut mXCorr: [f64; M_XCORR] = [0; M_XCORR];	// The length should not exceed 256.
	if (length > 384)
	{
		debug!("Error! Increase the size of mXCorr.\n");
		return 0;
	}

	for i in 1..(length * 2)
	{
		let n = i - length;
		mXCorr[i * 2] = 0;
		mXCorr[i * 2 + 1] = 0;


		if (n < 0)
		{
			let mut m = -n;
			while  (m + n < 160) && (m < length)
			{
				// (a-bi) * (c+di)
				// = ac + bd + i*(ad - bc)
				let a = mIQRawData[m * 2];
				let b = mIQRawData[m * 2 + 1];
				let c = mOriginalLTS[(m + n) * 2];
				let d = mOriginalLTS[(m + n) * 2 + 1];

				mXCorr[i * 2] += (a*c + b*d);
				mXCorr[i * 2 + 1] += (a*d - b*c);
				
				m = m+1;

			}
		}
		else
		{
			for m in 0..(160 - n)
			{
				// (a-bi) * (c+di)
				// = ac + bd + i*(ad - bc)
				let a = mIQRawData[m * 2];
				let b = mIQRawData[m * 2 + 1];
				let c = mOriginalLTS[(m + n) * 2];
				let d = mOriginalLTS[(m + n) * 2 + 1];

				mXCorr[i * 2] += (a*c + b*d);
				mXCorr[i * 2 + 1] += (a*d - b*c);

			}
		}


	}

	peak: i32 = 0;
	maxkey: f64 = 0;


	for i in (length-64)..(length +64)
	{


		if (mXCorr[i * 2] * mXCorr[i * 2] + mXCorr[i * 2 + 1] * mXCorr[i * 2 + 1] > maxkey)
		{
			maxkey = mXCorr[i * 2] * mXCorr[i * 2] + mXCorr[i * 2 + 1] * mXCorr[i * 2 + 1];
			peak = i;
		}



	}

	return length - peak;
}