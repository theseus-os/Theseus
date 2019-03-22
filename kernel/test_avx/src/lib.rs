#![no_std]
#![feature(alloc)]
#![feature(asm)]

extern crate alloc;
extern crate x86_64;
#[macro_use] extern crate log;
// #[macro_use] extern crate terminal_print;
// extern crate muFFT;
extern crate apic;
extern crate spawn;
// extern crate scheduler;


// use muFFT::trig::cos;
use core::arch::x86_64::*;
use core::mem;
use x86_64::registers::control_regs::{Cr4, cr4, cr4_write};

macro_rules! debugprint {
    ($fmt:expr) => (debug!(concat!("###AVX### ", $fmt)));
    ($fmt:expr, $($arg:tt)*) => (debug!(concat!("###AVX### ", $fmt), $($arg)*));
}

use alloc::string::String;

const OSXSAVE_MASK_IN_ECX: u32 = 1 << 27;
const AVX_MASK_IN_ECX: u32 = 1 << 28;
const AVX2_MASK_IN_EBX: u32 = 1 << 5;
const SSE_MASK_IN_XCR0: u64 = 1 << 1;
const AVX_MASK_IN_XCR0: u64 = 1 << 2;
const OSXSAVE_MASK_IN_CR4: usize = 1 << 18;

fn is_xsave_enabled() -> bool {
	unsafe {__cpuid_count(0x1, 0)}.ecx & OSXSAVE_MASK_IN_ECX != 0
}

fn is_sse_avx_enabled() -> (bool, bool) {
	let res = unsafe {_xgetbv(0)};
	return (res & SSE_MASK_IN_XCR0 != 0, res & AVX_MASK_IN_XCR0 != 0)
}

fn enable_sse_avx() -> bool {
	let mut res = unsafe {_xgetbv(0)};
	res |= SSE_MASK_IN_XCR0 | AVX_MASK_IN_XCR0;

	unsafe {_xsetbv(0, res)};

	is_sse_avx_enabled() == (true, true)
}

fn enable_avx(no_check: bool) -> bool{
    if no_check {debugprint!("Enable AVX anyway without checking CPU capability");}

	// proposed steps by Intel
	// 1) check XSAVE (OS)
	if no_check || !is_xsave_enabled() {
		if !no_check {debugprint!("OS-CFG-ERR: OS does not enables XSAVE");}

        debugprint!("OS-CFG: Trying to enable XSAVE...");
		let mut _cr4 = cr4().bits();
		let new_cr4 = Cr4::from_bits(_cr4 | OSXSAVE_MASK_IN_CR4).unwrap();
		unsafe {cr4_write(new_cr4)};

		if !is_xsave_enabled() {
			debugprint!("Cannot enable XSAVE");
			return false;
		}
	}
	debugprint!("XSAVE is enabled");

	// 2) check SSE/AVX (OS)
	let need_reconf;
	match is_sse_avx_enabled() {
		(false, false) => { debugprint!("OS-CFG-ERR: SSE and AVX are disalbed"); need_reconf = true; }
		(true, false) => { debugprint!("OS-CFG-ERR: AVX is disalbed"); need_reconf = true; }
		(false, true) => { debugprint!("OS-CFG-ERR: SSE is disalbed"); need_reconf = true; }
		_ => {need_reconf = false;}
	}
	if no_check || need_reconf {
        debugprint!("OS-CFG: Trying to enable SSE/AVX...");
		if !enable_sse_avx() {
			debugprint!("OS-CFG-ERR: Cannot enable SSE or AVX\n");
			return false;
		}
	}
	debugprint!("SSE and AVX are enabled");

	// check AVX (arch)
	let res = unsafe {__cpuid_count(0x1, 0)}.ecx;
	if res & AVX_MASK_IN_ECX == 0 {
		debugprint!("ARCH-ERR: Cannot run because the CPU does not support AVX");
		return false;
	}


	// for curiosity
	let res = unsafe {__cpuid_count(0x7, 0)}.ebx;
	if res & AVX2_MASK_IN_EBX == 0 {
		debugprint!("The CPU does not support AVX2 :(");
	} else { debugprint!("The CPU supports AVX2 :)"); }

	true
}


pub fn test_asm(arg: (f64, f64, f64, f64, &'static str, u64, f64)) {
    let mut ans_f64_check: [f64; 4] = [arg.0, arg.1, arg.2, arg.3];
    unsafe{
        asm!("
            push $0
            push $1
            push $2
            push $3
            vmovupd ymm15, [rsp]
            pop rax
            pop rax
            pop rax
            pop rax

            vmulpd ymm15, ymm15, ymm15
            "
            :: "r"(arg.0), "r"(arg.1), "r"(arg.2), "r"(arg.3)
            : "memory", "rax"
            : "intel", "volatile"
        );
    }

    // debugprint!("{} INITED", arg.4);
    // scheduler::schedule();

    // let ans0 = arg.0*arg.0;
    // let ans1 = arg.1*arg.1;
    // let ans2 = arg.2*arg.2;
    // let ans3 = arg.3*arg.3;

    // unsafe{
    //     asm!("
    //         vmovupd [$0], ymm15;
    //         "
    //         :: "r"(&ans_f64_check) : "memory" : "intel", "volatile"
    //     );
    // }

    // debugprint!("{} - before loop [{}, {}, {}, {}] = {:?}", 
    //     arg.4,
    //     (arg.0), (arg.1), (arg.2), (arg.3),
    //     ans_f64_check);

    // debugprint!("{} CHECK BEFORE LOOP - [{:3.2}, {:3.2}, {:3.2}, {:3.2}]", arg.4,
    //     ans_f64_check[0],
    //     ans_f64_check[1],
    //     ans_f64_check[2],
    //     ans_f64_check[3]);

    // let mut loop_ctr: u64 = 0;
    loop {
        // loop_ctr += 1;
        // ans_f64_check = [arg.0, arg.1, arg.2, arg.3];
        unsafe{
            asm!("
                vmovupd [$0], ymm15;
                "
                :: "r"(&ans_f64_check) : "memory" : "intel", "volatile"
            );
        }
        // if loop_ctr > 50_000_000 {
        // {
        //     debugprint!("{} [{}, {}, {}, {}] = {:?}", 
        //         arg.4,
        //         (arg.0), (arg.1), (arg.2), (arg.3),
        //         ans_f64_check);
        //     loop_ctr = 0;
        //     break;
        // }

        // if ans0 != ans_f64_check[0] || ans1 != ans_f64_check[1] ||
        //    ans2 != ans_f64_check[2] || ans3 != ans_f64_check[3] {

        // if arg.0*arg.0 != ans_f64_check[0] || arg.1*arg.1 != ans_f64_check[1] ||
        //    arg.2*arg.2 != ans_f64_check[2] || arg.3*arg.3 != ans_f64_check[3] {

        if arg.6 != ans_f64_check[0] || arg.6 != ans_f64_check[1] ||
           arg.6 != ans_f64_check[2] || arg.6 != ans_f64_check[3] {



            // debugprint!("{} CHECK - [{:3.2}, {:3.2}, {:3.2}, {:3.2}] - CTR_{}", arg.4,
            //     ans_f64_check[0],
            //     ans_f64_check[1],
            //     ans_f64_check[2],
            //     ans_f64_check[3],
            //     loop_ctr);

            debugprint!("{} CHECK - [{:3.2}, {:3.2}, {:3.2}, {:3.2}]", arg.4,
                ans_f64_check[0],
                ans_f64_check[1],
                ans_f64_check[2],
                ans_f64_check[3]);
            break;

        }
                 //         ans_f64_check);

    }

    debugprint!("{} existed because of YMM err", arg.4);
}

#[inline(never)]
fn avx_mul(arg: (f64, f64, f64, f64)) -> __m256d {
    let rtn = unsafe {
        let c = _mm256_setr_pd(arg.0, arg.1, arg.2, arg.3);
        let t = _mm256_setr_pd(arg.0, arg.1, arg.2, arg.3);
        _mm256_mul_pd(c, t)
    };

    rtn
}

pub fn test_core(arg: (f64, f64, f64, f64, &'static str, u64, f64)) {
    // let mut c;
    // let mut t;
    let mut ct;
    // unsafe {
    //     c = _mm256_setr_pd(arg.0, arg.1, arg.2, arg.3);
    //     t = _mm256_setr_pd(arg.0, arg.1, arg.2, arg.3);
    //     ct = _mm256_mul_pd(c, t);
    // }
    ct = avx_mul((arg.0, arg.1, arg.2, arg.3));

    debugprint!("{} - before loop [{}, {}, {}, {}] = {:?}", 
        arg.4,
        arg.0*arg.0, arg.1*arg.1, arg.2*arg.2, arg.3*arg.3,
        ct);

    let mut loop_ctr: u64 = 1;
    loop {
        loop_ctr += arg.5;
        // unsafe { 
        //     c = _mm256_setr_pd(arg.0, arg.1, arg.2, arg.3);
        //     t = _mm256_setr_pd(arg.0, arg.1, arg.2, arg.3);
        //     ct = _mm256_mul_pd(c, t);
        // }
        ct = avx_mul((arg.0, arg.1, arg.2, arg.3));
        // let ct_dummy = avx_mul((0.0, 0.0, 0.0, 0.0));
        let ct_unpacked: (f64, f64, f64, f64) = unsafe{ mem::transmute(ct) };
        // if loop_ctr > 50_000_000 {
            // if arg.0*arg.0 != ct_unpacked.0 || arg.1*arg.1 != ct_unpacked.1 ||
            //    arg.2*arg.2 != ct_unpacked.2 || arg.3*arg.3 != ct_unpacked.3 {
            if arg.6 != ct_unpacked.0 || arg.6 != ct_unpacked.1 ||
               arg.6 != ct_unpacked.2 || arg.6 != ct_unpacked.3 {
                debugprint!("{} - DIFFERS! [{}, {}, {}, {}] = {:?}", 
                    arg.4,
                    arg.0*arg.0, arg.1*arg.1, arg.2*arg.2, arg.3*arg.3,
                    ct);
            }

            debugprint!("{} CHECK [{:.2}, {:.2}, {:.2}, {:.2}]", 
                arg.4,
                ct_unpacked.0,
                ct_unpacked.1,
                ct_unpacked.2,
                ct_unpacked.3);
            // loop_ctr = 1;
        // }

        // if loop_ctr == 0 {
        //     // this will never happen!
        //     debugprint!("{:?}", ct_dummy);
        // }
    }
    debugprint!("{} - ENDS", arg.4);
}

pub fn auto_test(_: ()) -> Result<(), &'static str> {
    debugprint!("AUTO_TEST!");

    if !enable_avx(false/* no_check */) { return Err("Cannot enable AVX!"); }

    let this_core = apic::get_my_apic_id().ok_or("couldn't get my APIC id")?;

    let task1 = spawn::KernelTaskBuilder::new(test_asm, (3.1, 3.1, 3.1, 3.1, &"TEST_ASM_1", 1, 3.1*3.1))
        .name(String::from("AVX_TEST_1"))
        .pin_on_core(this_core)
        // .avx()?
        .spawn()?;

    let task2 = spawn::KernelTaskBuilder::new(test_asm, (1.9, 1.9, 1.9, 1.9, &"TEST_ASM_2", 3, 1.9*1.9))
        .name(String::from("AVX_TEST_2"))
        .pin_on_core(this_core)
        // .avx()?
        .spawn()?;
    
    let task3 = spawn::KernelTaskBuilder::new(test_asm, (10.4, 10.4, 10.4, 10.4, &"TEST_ASM_3", 5, 10.4*10.4))
        .name(String::from("AVX_TEST_3"))
        .pin_on_core(this_core)
        // .avx()?
        .spawn()?;
    
    /*let task1 = spawn::KernelTaskBuilder::new(test_core, (3.1, 3.1, 3.1, 3.1, &"TEST_CORE_1", 1))
        .name(String::from("AVX_TEST_1"))
        .pin_on_core(this_core)
        // .avx()?
        .spawn()?;

    let task2 = spawn::KernelTaskBuilder::new(test_core, (1.9, 1.9, 1.9, 1.9, &"TEST_CORE_2", 3))
        .name(String::from("AVX_TEST_2"))
        .pin_on_core(this_core)
        // .avx()?
        .spawn()?;

    let task3 = spawn::KernelTaskBuilder::new(test_core, (10.4, 10.4, 10.4, 10.4, &"TEST_CORE_3", 5))
        .name(String::from("AVX_TEST_3"))
        .pin_on_core(this_core)
        // .avx()?
        .spawn()?;*/

    loop { }
    
    // task1.join()?;
    // task2.join()?;
    // task3.join()?;

    Ok(())
}