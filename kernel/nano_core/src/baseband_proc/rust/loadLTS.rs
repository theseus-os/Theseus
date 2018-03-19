extern crate csv;
use std::io;

const LTS_SYMBOLS_ORIG:			usize = 160;
const LTS_SYMBOLS:				usize = 128;
const MASK_SIZE:				usize = 64;

static M_ORIGINAL_LTS: 			Once<[f64; LTS_SYMBOLS_ORIG]> = Once::new();
static M_LTS: 					Once<[f64; LTS_SYMBOLS]> = Once::new(); 
static M_MASK: 					Once<[i32; MASK_SIZE]> = Once::new(); 

/// LTS: Long Training Sequence
/// first 160 symbols are real parts and next 160 are imaginary
/// load into mOriginalLTS
/// perform DFT and find mLTS
/// find mask
pub fn loadLTSFromFile(file_path: String) {

    let file = File::open(file_path)?;
    let mut rdr = csv::Reader::from_reader(file);
    for result in rdr.records() {
        let record = result?;
        println!("{:?}", record);
    }
    Ok(())
	

	mOriginalLTS = (float*)malloc(sizeof(float) * 320);

	for (int i = 0; i < 160; i++)
	{
		fscanf(fp, "%f,", &(mOriginalLTS[i * 2]));
		//mOriginalLTS[i * 2] *= 0.333;


	}

	for (int i = 0; i < 160; i++)
	{
		fscanf(fp, "%f,", &(mOriginalLTS[i * 2 + 1]));
		//mOriginalLTS[i*2+1] *= 0.333;
	}

	fclose(fp);

	

	DFT(mOriginalLTS + 64, mLTS);



	for (int i = 0; i < 64; i++)
	{
		mMask[i] = 0;
		//printf("mLTS %f + %f i\n", mLTS[i * 2], mLTS[i * 2 + 1]);
		if (fabsf(mLTS[i * 2]) > 0.01 || fabsf(mLTS[i * 2 + 1]) > 0.01)
		{
			//printf("Mask %d\n",i);
			mMask[i] = 1;
		}
	}


}