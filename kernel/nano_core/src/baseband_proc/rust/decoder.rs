// sse2turbo.cpp : Defines the entry point for the console application.
//

/*
extern __m128i _mm_stream_load_si128(__m128i* v1);
Corresponding SSE4 instruction: MOVNTDQA
Loads _m128 data from a 16-byte aligned address (v1) to the destination operand
(m128i) without polluting the caches.
*/

#include <stdio.h>

#ifdef _WIN32
#include <SDKDDKVer.h>
#else
#include <inttypes.h>
#define __int64 int64_t
#endif

//#include "stdio.h"
//#include "encoder.cpp"
#include "timer.h"
//#include "inttypes.h"

//#define __int64 int64_t
//#include <x86intrin>
//#include <nmmintrin.h>
#include <immintrin.h>
#include <omp.h>


#define __forceinline inline


#define uint16_t short

#define NUMTHREADS 1

#define NUMITER 6

//#define NUMPACKETS 4096
#define NUMPACKETS 4096
#define NUMFRAC 5
#define NUMBITS 9


#define MAXVALUE 2047 //this is actually 11 bits

#define INITITER 0
#define FINALITER 1
#define SISO0 3
#define SISO1 4
#define NORMITER 99

#define LOGTYPE MAXLOG
#define MAXLOG 0
#define LOGMAP 1

#define ETON  0
#define ETOFF 0

#define ET ETON

//#define _mm_max_epi16 LinearLogMap
const __m128i kzero = _mm_setzero_si128();
const __m128i kC = _mm_set_epi16(177, 177, 177, 177, 177, 177, 177, 177); //6 bits
																		  //const __m128i kC	 =  _mm_set_epi16( 89, 89, 89, 89, 89, 89, 89, 89); //5 bits
																		  //	const __m128i kC	 =  _mm_set_epi16(44,44,44,44,44,44,44,44); //4 bits

template <int mode>
inline __m128i LinearLogMap(__m128i a, __m128i b) {

	__m128i maxterm = _mm_max_epi16(a, b);
	__m128i c;

	if (mode == MAXLOG) {
		c = maxterm;
	}
	else {
		//compute correction time
		__m128i z = _mm_sub_epi16(a, b);
		z = _mm_abs_epi16(z);


		__m128i cterm = _mm_sub_epi16(kC, z);
		cterm = _mm_srai_epi16(cterm, 2);
		cterm = _mm_max_epi16(cterm, kzero);

		c = _mm_add_epi16(maxterm, cterm);
	}

	return c;
}

void print128_num(__m128i var)
{
	uint16_t *val = (uint16_t*)&var;
	//printf("Numerical: %i %i %i %i %i %i %i %i \n", 
	printf("\t%i %i %i %i %i %i %i %i \n",
		val[0], val[1], val[2], val[3], val[4], val[5],
		val[6], val[7]);
}

//lazy update
inline __int64 updateCRC(__m128i in, __int64 crc) {
	//compute crc
	__m128i bits = _mm_cmpgt_epi16(kzero, in);

	bits = _mm_packs_epi16(bits, bits);
	__int64 top = _mm_extract_epi64(bits, 0);
	//__int64 btm = _mm_extract_epi64(bits,1);



	crc = _mm_crc32_u64(crc, top);
	//crc = _mm_crc32_u64(crc,btm);

	return crc;
}

inline void ScatterWrite(short * arr, __m128i src, __m128i addr) {

	int wraddr;

	wraddr = _mm_extract_epi16(addr, 0);
	arr[wraddr] = _mm_extract_epi16(src, 0);
	wraddr = _mm_extract_epi16(addr, 1);
	arr[wraddr] = _mm_extract_epi16(src, 1);
	wraddr = _mm_extract_epi16(addr, 2);
	arr[wraddr] = _mm_extract_epi16(src, 2);
	wraddr = _mm_extract_epi16(addr, 3);
	arr[wraddr] = _mm_extract_epi16(src, 3);
	wraddr = _mm_extract_epi16(addr, 4);
	arr[wraddr] = _mm_extract_epi16(src, 4);
	wraddr = _mm_extract_epi16(addr, 5);
	arr[wraddr] = _mm_extract_epi16(src, 5);
	wraddr = _mm_extract_epi16(addr, 6);
	arr[wraddr] = _mm_extract_epi16(src, 6);
	wraddr = _mm_extract_epi16(addr, 7);
	arr[wraddr] = _mm_extract_epi16(src, 7);
}

inline __m128i ThresholdVec(__m128i src) {
	const __m128i maxvec = _mm_set_epi16(MAXVALUE, MAXVALUE, MAXVALUE, MAXVALUE, MAXVALUE, MAXVALUE, MAXVALUE, MAXVALUE);
	const __m128i minvec = _mm_set_epi16(-MAXVALUE, -MAXVALUE, -MAXVALUE, -MAXVALUE, -MAXVALUE, -MAXVALUE, -MAXVALUE, -MAXVALUE);

	__m128i outval;
	outval = _mm_min_epi16(src, maxvec);
	outval = _mm_max_epi16(outval, minvec);

	return outval;

}

template <int mode>
__forceinline  __m128i HMaxVec(__m128i * in) {
	__m128i shuffle0;
	__m128i shuffle1;
	__m128i shuffle2;
	__m128i shuffle3;
	__m128i shuffle4;
	__m128i shuffle5;
	__m128i shuffle6;
	__m128i shuffle7;


	const __m128i pattern0 = _mm_set_epi8(7, 6, 5, 4, 3, 2, 1, 0, 15, 14, 13, 12, 11, 10, 9, 8); //checked

																								 //input
																								 /*	printf("input\n");
																								 print128_num(in[0]);
																								 print128_num(in[1]);
																								 print128_num(in[2]);
																								 print128_num(in[3]);
																								 print128_num(in[4]);
																								 print128_num(in[5]);
																								 print128_num(in[6]);
																								 print128_num(in[7]);*/

																								 //level 0
																								 //	printf("level 0\n");
	shuffle0 = _mm_blend_epi16(in[0], in[1], 0xF0);
	shuffle1 = _mm_blend_epi16(in[1], in[0], 0xF0);
	shuffle1 = _mm_shuffle_epi8(shuffle1, pattern0);

	shuffle2 = _mm_blend_epi16(in[2], in[3], 0xF0);
	shuffle3 = _mm_blend_epi16(in[3], in[2], 0xF0);
	shuffle3 = _mm_shuffle_epi8(shuffle3, pattern0);


	shuffle4 = _mm_blend_epi16(in[4], in[5], 0xF0);
	shuffle5 = _mm_blend_epi16(in[5], in[4], 0xF0);
	shuffle5 = _mm_shuffle_epi8(shuffle5, pattern0);

	shuffle6 = _mm_blend_epi16(in[6], in[7], 0xF0);
	shuffle7 = _mm_blend_epi16(in[7], in[6], 0xF0);
	shuffle7 = _mm_shuffle_epi8(shuffle7, pattern0);

	shuffle0 = LinearLogMap<mode>(shuffle0, shuffle1);
	shuffle2 = LinearLogMap<mode>(shuffle2, shuffle3);
	shuffle4 = LinearLogMap<mode>(shuffle4, shuffle5);
	shuffle6 = LinearLogMap<mode>(shuffle6, shuffle7);
	/*	print128_num(shuffle0);
	print128_num(shuffle2);
	print128_num(shuffle4);
	print128_num(shuffle6);
	*/

	//	printf("level 1\n");
	//level1
	//delete the next four lines
	/*shuffle0 =  _mm_set_epi16(7,6,5,4,3,2,1,0);
	shuffle2 =  _mm_set_epi16(15,14,13,12,11,10,9,8);
	print128_num(shuffle0);
	print128_num(shuffle2);*/

	const __m128i pattern1 = _mm_set_epi8(11, 10, 9, 8, 15, 14, 13, 12, 3, 2, 1, 0, 7, 6, 5, 4); //checked
	shuffle1 = _mm_blend_epi16(shuffle0, shuffle2, 0x33);
	shuffle0 = _mm_blend_epi16(shuffle2, shuffle0, 0x33);
	shuffle1 = _mm_shuffle_epi8(shuffle1, pattern1);
	//	print128_num(shuffle0);
	//	print128_num(shuffle1);

	shuffle2 = _mm_blend_epi16(shuffle4, shuffle6, 0x33);
	shuffle3 = _mm_blend_epi16(shuffle6, shuffle4, 0x33);
	shuffle3 = _mm_shuffle_epi8(shuffle3, pattern1);
	//	print128_num(shuffle2);
	//	print128_num(shuffle3);

	shuffle0 = LinearLogMap<mode>(shuffle0, shuffle1);
	shuffle2 = LinearLogMap<mode>(shuffle2, shuffle3);
	//	print128_num(shuffle0);
	//	print128_num(shuffle2);

	//level2
	//	printf("level 2\n");
	shuffle1 = _mm_blend_epi16(shuffle0, shuffle2, 0x55);
	shuffle0 = _mm_blend_epi16(shuffle2, shuffle0, 0x55);
	//	print128_num(shuffle0);
	//	print128_num(shuffle1);

	const __m128i pattern2 = _mm_set_epi8(13, 12, 15, 14, 9, 8, 11, 10, 5, 4, 7, 6, 1, 0, 3, 2); //checked
	shuffle1 = _mm_shuffle_epi8(shuffle1, pattern2);
	//	print128_num(shuffle1);
	shuffle0 = LinearLogMap<mode>(shuffle0, shuffle1);
	//	print128_num(shuffle0);

	//level3, last shuffle
	const __m128i pattern3 = _mm_set_epi8(11, 10, 3, 2, 15, 14, 7, 6, 13, 12, 5, 4, 9, 8, 1, 0);
	shuffle0 = _mm_shuffle_epi8(shuffle0, pattern3);
	//	print128_num(shuffle0);
	/*print128_num(in[0]);
	print128_num(in[1]);
	print128_num(shuffle0);
	print128_num(shuffle1);

	print128_num(in[2]);
	print128_num(in[3]);
	print128_num(shuffle2);
	print128_num(shuffle3);

	print128_num(in[4]);
	print128_num(in[5]);
	print128_num(shuffle4);
	print128_num(shuffle5);

	print128_num(in[6]);
	print128_num(in[7]);
	print128_num(shuffle6);
	print128_num(shuffle7);*/

	return shuffle0;
}
/*__m128i HMaxStar(__m128i in){

__m128i in_inv;
__m128i out;
in_inv = _mm_sub_epi16(_mm_set1_epi16(0), in); //invert the input

//find minimum
__m128i min_pos;
__m128i min_neg;
min_pos = _mm_minpos_epu16(in_inv);//find the minimum of the postive numbers
in_inv = _mm_xor_si128(_mm_set1_epi16(0x8000), in_inv);
min_neg = _mm_minpos_epu16(in_inv);//find the minimum of the negative numbers
min_neg = _mm_xor_si128(_mm_set1_epi16(0x8000), min_neg);
out = _mm_min_epi16(min_pos, min_neg);

//invert output
out = _mm_sub_epi16(_mm_set1_epi16(0), out);
return out;
}*/

/*** checked ***/
template <int mode>
inline __m128i ComputeAlpha(__m128i gamma, __m128i alpha) {

	const __m128i alpha_mask_pos = _mm_set_epi8(15, 14, 9, 8, 7, 6, 1, 0, 13, 12, 11, 10, 5, 4, 3, 2); //checked
	const __m128i alpha_mask_neg = _mm_set_epi8(13, 12, 11, 10, 5, 4, 3, 2, 15, 14, 9, 8, 7, 6, 1, 0); //checked



	__m128i alpha_next; //this is the output

	__m128i alpha_pos;
	__m128i alpha_neg;
	alpha_pos = _mm_add_epi16(alpha, gamma);
	alpha_neg = _mm_sub_epi16(alpha, gamma);

	__m128i alpha_pos_shuffle;
	__m128i alpha_neg_shuffle;
	alpha_pos_shuffle = _mm_shuffle_epi8(alpha_pos, alpha_mask_pos);
	alpha_neg_shuffle = _mm_shuffle_epi8(alpha_neg, alpha_mask_neg);

	alpha_next = LinearLogMap<mode>(alpha_pos_shuffle, alpha_neg_shuffle);

	//normalization, once per iteration, change it to one time every 8 iterations
	/*
	const __m128i top_mask =  _mm_set_epi8(1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0);
	__m128i normfactor = _mm_shuffle_epi8(alpha_next,top_mask);
	alpha_next = _mm_sub_epi16(alpha_next, normfactor);
	*/

	return alpha_next;

}

/*** checked ***/
template <int mode>
inline __m128i ComputeBeta(__m128i * beta_pos_ptr, __m128i * beta_neg_ptr, __m128i gamma, __m128i beta) {


	const __m128i beta_mask_pos = _mm_set_epi8(15, 14, 7, 6, 5, 4, 13, 12, 11, 10, 3, 2, 1, 0, 9, 8); //checked
	const __m128i beta_mask_neg = _mm_set_epi8(7, 6, 15, 14, 13, 12, 5, 4, 3, 2, 11, 10, 9, 8, 1, 0); //checked

	__m128i beta_next; //this is the output

	__m128i beta_pos;
	__m128i beta_neg;
	beta_pos = _mm_add_epi16(beta, gamma);
	beta_neg = _mm_sub_epi16(beta, gamma);


	//compute next beta
	__m128i beta_pos_shuffle;
	__m128i beta_neg_shuffle;
	beta_pos_shuffle = _mm_shuffle_epi8(beta_pos, beta_mask_pos);
	beta_neg_shuffle = _mm_shuffle_epi8(beta_neg, beta_mask_neg);

	//store beta_pos and beta_neg;
	*beta_pos_ptr = beta_pos_shuffle;
	*beta_neg_ptr = beta_neg_shuffle;

	beta_next = LinearLogMap<mode>(beta_pos_shuffle, beta_neg_shuffle);

	//normalization, once per iteration, change it to one time every 8 iterations
	/*const __m128i top_mask =  _mm_set_epi8(1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0);
	__m128i normfactor = _mm_shuffle_epi8(beta_next,top_mask);
	beta_next = _mm_sub_epi16(beta_next, normfactor);*/


	//compute next llr;

	return beta_next;

}

//note, the number of stage must be divisible by 8. 
template <int pass, int mode>
int AlphaPass(__m128i * alpha_out, const __m128i * lsys, const __m128i * lp, const __m128i * la, __m128i alpha_init, int num_stage) {
	const __m128i gamma_init_mask = _mm_set_epi8(1, 0, 1, 0, 3, 2, 3, 2, 3, 2, 3, 2, 1, 0, 1, 0); //checked
	const __m128i inc = _mm_set_epi8(4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4);
	__m128i gamma_mask[4];

	alpha_out[0] = alpha_init;

	gamma_mask[0] = gamma_init_mask;
	gamma_mask[1] = _mm_add_epi8(gamma_mask[0], inc);
	gamma_mask[2] = _mm_add_epi8(gamma_mask[1], inc);
	gamma_mask[3] = _mm_add_epi8(gamma_mask[2], inc);


	const __m128i top_mask = _mm_set_epi8(1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0);

	__m128i alpha_imm[8];
	alpha_imm[0] = alpha_init;

	for (int i = 0; i < num_stage / 8; i++) {


		__m128i sum, gamma0, gamma1, gamma_right, gamma_left;
		if (pass == INITITER) {
			sum = lsys[i];
		}
		else if (pass == SISO1) {
			sum = la[i];
		}
		else {
			sum = _mm_add_epi16(la[i], lsys[i]);
		}

		gamma1 = _mm_add_epi16(sum, lp[i]);
		gamma0 = _mm_sub_epi16(sum, lp[i]);
		//gamma1 = _mm_srai_epi16(gamma1,1); //divide by 2
		//gamma0 = _mm_srai_epi16(gamma0,1);

		gamma_right = _mm_unpackhi_epi16(gamma1, gamma0);
		gamma_left = _mm_unpacklo_epi16(gamma1, gamma0);

		__m128i gamma[8];

		//working from left to right
		//print128_num(gamma_left);
		gamma[0] = _mm_shuffle_epi8(gamma_left, gamma_mask[0]);
		//print128_num(gamma[0]);

		gamma[1] = _mm_shuffle_epi8(gamma_left, gamma_mask[1]);
		gamma[2] = _mm_shuffle_epi8(gamma_left, gamma_mask[2]);
		gamma[3] = _mm_shuffle_epi8(gamma_left, gamma_mask[3]);
		gamma[4] = _mm_shuffle_epi8(gamma_right, gamma_mask[0]);
		gamma[5] = _mm_shuffle_epi8(gamma_right, gamma_mask[1]);
		gamma[6] = _mm_shuffle_epi8(gamma_right, gamma_mask[2]);
		gamma[7] = _mm_shuffle_epi8(gamma_right, gamma_mask[3]);




		//print128_num(gamma[1]);
		//print128_num(gamma[2]);
		//print128_num(gamma[3]);

		//exit(0);


		alpha_imm[1] = ComputeAlpha<mode>(gamma[0], alpha_imm[0]);
		alpha_out[i * 8 + 1] = alpha_imm[1];
		alpha_imm[2] = ComputeAlpha<mode>(gamma[1], alpha_imm[1]);
		alpha_out[i * 8 + 2] = alpha_imm[2];
		alpha_imm[3] = ComputeAlpha<mode>(gamma[2], alpha_imm[2]);
		alpha_out[i * 8 + 3] = alpha_imm[3];
		alpha_imm[4] = ComputeAlpha<mode>(gamma[3], alpha_imm[3]);
		alpha_out[i * 8 + 4] = alpha_imm[4];
		alpha_imm[5] = ComputeAlpha<mode>(gamma[4], alpha_imm[4]);
		alpha_out[i * 8 + 5] = alpha_imm[5];
		alpha_imm[6] = ComputeAlpha<mode>(gamma[5], alpha_imm[5]);
		alpha_out[i * 8 + 6] = alpha_imm[6];
		alpha_imm[7] = ComputeAlpha<mode>(gamma[6], alpha_imm[6]);
		alpha_imm[0] = ComputeAlpha<mode>(gamma[7], alpha_imm[7]);
		alpha_out[i * 8 + 7] = alpha_imm[7];

		__m128i normfactor = _mm_shuffle_epi8(alpha_imm[0], top_mask);
		alpha_imm[0] = _mm_sub_epi16(alpha_imm[0], normfactor);
		alpha_out[i * 8 + 8] = alpha_imm[0];

	}

	return 0;
}

//note, the number of stage must be divisible by 8. 
template <int pass, int mode, int et>
__int64 BetaPass(__m128i * memout, const __m128i * addr, const __m128i * lsys, const __m128i * lp, const __m128i * la, const __m128i * alpha, __m128i beta_init, int num_stage) {
	const __m128i gamma_init_mask = _mm_set_epi8(1, 0, 3, 2, 3, 2, 1, 0, 1, 0, 3, 2, 3, 2, 1, 0); //checked
	const __m128i inc = _mm_set_epi8(4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4);
	__m128i gamma_mask[4];

	gamma_mask[0] = gamma_init_mask;
	gamma_mask[1] = _mm_add_epi8(gamma_mask[0], inc);
	gamma_mask[2] = _mm_add_epi8(gamma_mask[1], inc);
	gamma_mask[3] = _mm_add_epi8(gamma_mask[2], inc);


	const __m128i top_mask = _mm_set_epi8(1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0);
	//const __m128i maxlogscale = _mm_set_epi16(11,11,11,11,11,11,11,11); //this is 4 bits
	//const __m128i maxlogscale = _mm_set_epi16(22,22,22,22,22,22,22,22); //this is 5 bits


	__int64 crc = 0;

	__m128i beta_imm[8];
	beta_imm[0] = beta_init;
	//print128_num(beta_imm[0]);
	//for (int i = 0; i < num_stage/8; i++){
	for (int i = num_stage / 8 - 1; i >= 0; i--) {
		//printf("i = %d\n", i);

		__m128i sum, gamma0, gamma1, gamma_right, gamma_left;
		if (pass == INITITER) {
			sum = lsys[i];
		}
		else if ((pass == SISO1) | (pass == FINALITER)) {
			sum = la[i];
		}
		else {
			sum = _mm_add_epi16(la[i], lsys[i]);
		}
		gamma1 = _mm_add_epi16(sum, lp[i]);
		gamma0 = _mm_sub_epi16(sum, lp[i]);
		/*gamma1 = _mm_srai_epi16(gamma1,1); //divide by 2
		gamma0 = _mm_srai_epi16(gamma0,1);*/

		/*printf("gamma1: \n");
		print128_num(gamma1);
		printf("gamma0: \n");
		print128_num(gamma0);*/

		gamma_right = _mm_unpackhi_epi16(gamma1, gamma0);
		gamma_left = _mm_unpacklo_epi16(gamma1, gamma0);

		__m128i gamma[8];

		//working from left to right
		gamma[7] = _mm_shuffle_epi8(gamma_left, gamma_mask[0]);
		gamma[6] = _mm_shuffle_epi8(gamma_left, gamma_mask[1]);
		gamma[5] = _mm_shuffle_epi8(gamma_left, gamma_mask[2]);
		gamma[4] = _mm_shuffle_epi8(gamma_left, gamma_mask[3]);
		gamma[3] = _mm_shuffle_epi8(gamma_right, gamma_mask[0]);
		gamma[2] = _mm_shuffle_epi8(gamma_right, gamma_mask[1]);
		gamma[1] = _mm_shuffle_epi8(gamma_right, gamma_mask[2]);
		gamma[0] = _mm_shuffle_epi8(gamma_right, gamma_mask[3]);

		__m128i beta_pos[8], beta_neg[8];

		beta_imm[1] = ComputeBeta<mode>(&beta_pos[0], &beta_neg[0], gamma[0], beta_imm[0]);
		/*printf("beta: ");
		print128_num(beta_imm[0]);
		printf("alpha[%d]: ", i*8+7);
		print128_num(alpha[i*8+7]);*/
		beta_imm[2] = ComputeBeta<mode>(&beta_pos[1], &beta_neg[1], gamma[1], beta_imm[1]);
		beta_imm[3] = ComputeBeta<mode>(&beta_pos[2], &beta_neg[2], gamma[2], beta_imm[2]);
		beta_imm[4] = ComputeBeta<mode>(&beta_pos[3], &beta_neg[3], gamma[3], beta_imm[3]);
		beta_imm[5] = ComputeBeta<mode>(&beta_pos[4], &beta_neg[4], gamma[4], beta_imm[4]);
		beta_imm[6] = ComputeBeta<mode>(&beta_pos[5], &beta_neg[5], gamma[5], beta_imm[5]);
		beta_imm[7] = ComputeBeta<mode>(&beta_pos[6], &beta_neg[6], gamma[6], beta_imm[6]);
		beta_imm[0] = ComputeBeta<mode>(&beta_pos[7], &beta_neg[7], gamma[7], beta_imm[7]);

		//normalize beta once every 8 iterations
		__m128i normfactor = _mm_shuffle_epi8(beta_imm[0], top_mask);
		beta_imm[0] = _mm_sub_epi16(beta_imm[0], normfactor);

		/*		printf("beta[%d]: ", i*8+7);
		print128_num(beta_imm[1]);
		printf("beta[%d]: ", i*8+6);
		print128_num(beta_imm[2]);
		printf("beta[%d]: ", i*8+5);
		print128_num(beta_imm[3]);
		printf("beta[%d]: ", i*8+4);
		print128_num(beta_imm[4]);
		printf("beta[%d]: ", i*8+3);
		print128_num(beta_imm[5]);
		printf("beta[%d]: ", i*8+2);
		print128_num(beta_imm[6]);
		printf("beta[%d]: ", i*8+1);
		print128_num(beta_imm[7]);
		printf("beta[%d]: ", i*8+0);
		print128_num(beta_imm[0]); */

		__m128i sum_pos[8], sum_neg[8];
		sum_pos[7] = _mm_add_epi16(beta_pos[0], alpha[i * 8 + 7]);
		sum_pos[6] = _mm_add_epi16(beta_pos[1], alpha[i * 8 + 6]);
		sum_pos[5] = _mm_add_epi16(beta_pos[2], alpha[i * 8 + 5]);
		sum_pos[4] = _mm_add_epi16(beta_pos[3], alpha[i * 8 + 4]);
		sum_pos[3] = _mm_add_epi16(beta_pos[4], alpha[i * 8 + 3]);
		sum_pos[2] = _mm_add_epi16(beta_pos[5], alpha[i * 8 + 2]);
		sum_pos[1] = _mm_add_epi16(beta_pos[6], alpha[i * 8 + 1]);
		sum_pos[0] = _mm_add_epi16(beta_pos[7], alpha[i * 8 + 0]);

		sum_neg[7] = _mm_add_epi16(beta_neg[0], alpha[i * 8 + 7]);
		sum_neg[6] = _mm_add_epi16(beta_neg[1], alpha[i * 8 + 6]);
		sum_neg[5] = _mm_add_epi16(beta_neg[2], alpha[i * 8 + 5]);
		sum_neg[4] = _mm_add_epi16(beta_neg[3], alpha[i * 8 + 4]);
		sum_neg[3] = _mm_add_epi16(beta_neg[4], alpha[i * 8 + 3]);
		sum_neg[2] = _mm_add_epi16(beta_neg[5], alpha[i * 8 + 2]);
		sum_neg[1] = _mm_add_epi16(beta_neg[6], alpha[i * 8 + 1]);
		sum_neg[0] = _mm_add_epi16(beta_neg[7], alpha[i * 8 + 0]);


		__m128i pos, neg;
		pos = HMaxVec<mode>(sum_pos);
		neg = HMaxVec<mode>(sum_neg);

		__m128i APP;
		APP = _mm_sub_epi16(pos, neg);
		APP = _mm_srai_epi16(APP, 1);

		//update crc
		if ((pass == INITITER) | (pass == SISO0)&(et == ETON)) {
			crc = updateCRC(APP, crc);
		}

		__m128i le;
		if (pass == INITITER) {
			le = _mm_sub_epi16(APP, lsys[i]);
		}
		else if (pass == SISO0) {
			le = _mm_sub_epi16(APP, la[i]);
			le = _mm_sub_epi16(le, lsys[i]);
		}
		else if (pass == SISO1) {
			le = _mm_sub_epi16(APP, la[i]);
		}
		else if (pass == FINALITER) {
			le = APP;
		}

		if (pass != FINALITER) {

			if (mode == MAXLOG) {
				/*le = _mm_mullo_epi16(le, maxlogscale); //multiply method
				le = _mm_srai_epi16(le,NUMFRAC);*/

				//constant multiply by ~0.7
				__m128i shiftby1 = _mm_srai_epi16(le, 1);
				__m128i shiftby3 = _mm_srai_epi16(le, 3);
				__m128i shiftby4 = _mm_srai_epi16(le, 4);

				le = _mm_add_epi16(shiftby1, shiftby3);
				le = _mm_add_epi16(le, shiftby4);

				//variable multiply

				//__m128i intercept = _mm_set1_epi16(20);
				/*				__m128i intercept = _mm_set1_epi16(26);
				__m128i delta = _mm_set1_epi16(6);
				__m128i le_abs = _mm_abs_epi16(le);



				__m128i shiftby6 = _mm_srai_epi16(le_abs,6);
				__m128i shiftby7 = _mm_srai_epi16(le_abs,7);
				__m128i shiftby8 = _mm_srai_epi16(le_abs,8);

				__m128i slope = _mm_add_epi16(shiftby6,shiftby7);
				slope = _mm_add_epi16(slope,shiftby8);

				__m128i scale = _mm_add_epi16(slope,intercept);
				const __m128i maxscale = _mm_set1_epi16(32);
				scale = _mm_min_epi16(maxscale,scale);
				scale = _mm_sub_epi16(scale,delta);

				le = _mm_mullo_epi16(le, scale); //multiply method
				le = _mm_srai_epi16(le,NUMFRAC);
				*/

			}

			le = ThresholdVec(le);

			if (pass == SISO0) {
				le = _mm_add_epi16(le, lsys[i]);
			}
		}
		/*printf("llr :\n");
		print128_num(alpha[i*8+7]);
		print128_num(sum_pos[7]);
		print128_num(sum_neg[7]);
		print128_num(llr);*/

		//scatter write
		ScatterWrite((short *)memout, le, addr[i]);

	}

	return crc;
}

template <int mode>
__m128i ComputeInitBeta(const short * Ltbits) {

	//__m128i beta_init  = _mm_set_epi16 ((short)-2048,(short)-2048,(short)-2048,(short)-2048,(short)-2048,(short)-2048,(short)-2048,(short)0); 
	const __m128i beta_init = _mm_set_epi16((short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)0);

	__m128i gamma[3];

	for (int i = 0; i < 3; i++) {
		short lc_sys = Ltbits[2 * (2 - i)];
		short lc_par = Ltbits[2 * (2 - i) + 1];
		//short gamma10 = (lc_sys - lc_par)>>1;
		//short gamma11 = (lc_sys + lc_par)>>1;
		short gamma10 = (lc_sys - lc_par);
		short gamma11 = (lc_sys + lc_par);

		gamma[i] = _mm_set_epi16(gamma11, gamma10, gamma10, gamma11, gamma11, gamma10, gamma10, gamma11);
	}

	__m128i beta_imm[4];
	beta_imm[0] = beta_init;

	__m128i beta_pos[3], beta_neg[3];

	beta_imm[1] = ComputeBeta<mode>(&beta_pos[0], &beta_neg[0], gamma[0], beta_imm[0]);
	beta_imm[2] = ComputeBeta<mode>(&beta_pos[1], &beta_neg[1], gamma[1], beta_imm[1]);
	beta_imm[3] = ComputeBeta<mode>(&beta_pos[2], &beta_neg[2], gamma[2], beta_imm[2]);

	return beta_imm[3];

}

//La1 is the output
template <int mode, int et>
int TurboDecoder(short * La1, const short * Lch, const short * Lp0, const short *Lp1, const short * Ltbits, const short * InterleaverLUT, const short * DeinterleaverLUT, int K, int max_iter) {
	const __m128i alpha_init = _mm_set_epi16((short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)-8191, (short)0);
	//__m128i beta_init  = _mm_set_epi16 ((short)-33,(short)-33,(short)-33,(short)-33,(short)-33,(short)-33,(short)-33,(short)-33); 
	__m128i alpha_buffer[6145]; //init buffer
	__m128i La0[6144 / 8];

	__m128i beta_init_0 = ComputeInitBeta<mode>(Ltbits);
	__m128i beta_init_1 = ComputeInitBeta<mode>(Ltbits + 6);

	int num_iter = 0;
	__int64 crc = 0;
	__int64 crc_prev = 0;



	if (max_iter == 1) {
		AlphaPass<INITITER, mode>(alpha_buffer, (__m128i *) Lch, (__m128i *) Lp0, (__m128i *) La1, alpha_init, K);
		BetaPass<INITITER, mode, et>((__m128i *) La0, (__m128i *) DeinterleaverLUT, (__m128i *) Lch, (__m128i *) Lp0, (__m128i *) La1, alpha_buffer, beta_init_0, K);

		AlphaPass<SISO1, mode>(alpha_buffer, (__m128i *) Lch, (__m128i *) Lp1, (__m128i *) La0, alpha_init, K);
		BetaPass<FINALITER, mode, et>((__m128i *) La1, (__m128i *) InterleaverLUT, (__m128i *) Lch, (__m128i *) Lp1, (__m128i *) La0, alpha_buffer, beta_init_1, K);

		num_iter++;
	}
	else
	{
		AlphaPass<INITITER, mode>(alpha_buffer, (__m128i *) Lch, (__m128i *) Lp0, (__m128i *) La1, alpha_init, K);
		crc = BetaPass<INITITER, mode, et>((__m128i *) La0, (__m128i *) DeinterleaverLUT, (__m128i *) Lch, (__m128i *) Lp0, (__m128i *) La1, alpha_buffer, beta_init_0, K);


		for (int i = 1; i < max_iter; i++) {

			AlphaPass<SISO1, mode>(alpha_buffer, (__m128i *) Lch, (__m128i *) Lp1, (__m128i *) La0, alpha_init, K);
			BetaPass<SISO1, mode, et>((__m128i *) La1, (__m128i *) InterleaverLUT, (__m128i *) Lch, (__m128i *) Lp1, (__m128i *) La0, alpha_buffer, beta_init_1, K);
			num_iter++;

			crc_prev = crc;
			AlphaPass<SISO0, mode>(alpha_buffer, (__m128i *) Lch, (__m128i *) Lp0, (__m128i *) La1, alpha_init, K);
			crc = BetaPass<SISO0, mode, et>((__m128i *) La0, (__m128i *) DeinterleaverLUT, (__m128i *) Lch, (__m128i *) Lp0, (__m128i *) La1, alpha_buffer, beta_init_0, K);


			if ((crc_prev == crc)&(et == ETON)) {
				//ET = 1;
				break;
			}
		}

		/*if (ET == 1){
		short * La0Ptr = (short *) La0;
		for (int i = 0; i < K; i++){
		La1[i] = La0Ptr[DeinterleaverLUT[i]]+La1[i];
		}
		}
		else {*/
		AlphaPass<SISO1, mode>(alpha_buffer, (__m128i *) Lch, (__m128i *) Lp1, (__m128i *) La0, alpha_init, K);
		BetaPass<FINALITER, mode, et>((__m128i *) La1, (__m128i *) InterleaverLUT, (__m128i *) Lch, (__m128i *) Lp1, (__m128i *) La0, alpha_buffer, beta_init_1, K);
		num_iter++;
		//}
	}

	return num_iter;
}


extern "C"
{
	int CTurboDecoder(short * La1, const short * Lch, const short * Lp0, const short *Lp1, const short * Ltbits, const short * InterleaverLUT, const short * DeinterleaverLUT, int K, int max_iter)
	{
		return TurboDecoder<LOGTYPE, ET>(La1, Lch, Lp0, Lp1, Ltbits, InterleaverLUT, DeinterleaverLUT, K, max_iter);
	}

	
}



