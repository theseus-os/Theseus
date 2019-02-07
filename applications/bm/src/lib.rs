#![no_std]
#![feature(alloc)]

#[macro_use] extern crate alloc;
extern crate task;
extern crate acpi;
#[macro_use] extern crate terminal_print;
// #[macro_use] extern crate log;
extern crate fs_node;
extern crate apic;
extern crate spawn;
extern crate path;
extern crate runqueue;
extern crate memfs;

use core::str;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use acpi::get_hpet;
use memfs::MemFile;
use path::Path;
use fs_node::{DirRef, FileOrDir, FileRef};

// const ITERATIONS: usize = 1_000_000;
// const TRIES: usize = 10;
const THRESHOLD_ERROR_RATIO: u64 = 1;
const DIVISOR_FEMTO_TO_MICRO: u64 = 1_000_000_000;
const DIVISOR_FEMTO_TO_NANO: u64 = 1_000_000;

// for testing..
const ITERATIONS: usize = 1_000;
const TRIES: usize = 10;

// don't change it. 
const READ_BUF_SIZE: usize = 64*1024;

#[cfg(bm_in_us)]
const T_UNIT: &str = "micro sec";

#[cfg(not(bm_in_us))]
const T_UNIT: &str = "nano sec";

/*macro_rules! printlninfo {
    ($fmt:expr) => (warn!(concat!("BM-INFO: ", $fmt)));
    ($fmt:expr, $($arg:tt)*) => (warn!(concat!("BM-INFO: ", $fmt), $($arg)*));
}

macro_rules! printlnwarn {
    ($fmt:expr) => (warn!(concat!("BM-WARN: ", $fmt)));
    ($fmt:expr, $($arg:tt)*) => (warn!(concat!("BM-WARN: ", $fmt), $($arg)*));
}*/

macro_rules! printlninfo {
    ($fmt:expr) => (println!(concat!("BM-INFO: ", $fmt)));
    ($fmt:expr, $($arg:tt)*) => (println!(concat!("BM-INFO: ", $fmt), $($arg)*));
}

macro_rules! printlnwarn {
    ($fmt:expr) => (println!(concat!("BM-WARN: ", $fmt)));
    ($fmt:expr, $($arg:tt)*) => (println!(concat!("BM-WARN: ", $fmt), $($arg)*));
}

macro_rules! CPU_ID {
	() => (apic::get_my_apic_id().unwrap())
}

fn get_prog_name() -> String {
	let taskref = match task::get_my_current_task() {
        Some(t) => t,
        None => {
            printlninfo!("failed to get current task");
            return "Unknown".to_string();
        }
    };

    let locked_task = taskref.lock();
    locked_task.name.clone()
}

fn getpid() -> usize {
	let taskref = match task::get_my_current_task() {
        Some(t) => t,
        None => {
            printlninfo!("failed to get current task");
            return 0;
        }
    };

    let locked_task = taskref.lock();
    locked_task.id
}

fn hpet_2_us(hpet: u64) -> u64 {
	let hpet_period = get_hpet().as_ref().unwrap().counter_period_femtoseconds();
	hpet * hpet_period as u64 / DIVISOR_FEMTO_TO_MICRO
}

fn hpet_2_ns(hpet: u64) -> u64 {
	let hpet_period = get_hpet().as_ref().unwrap().counter_period_femtoseconds();
	let rtn = hpet * hpet_period as u64 / DIVISOR_FEMTO_TO_NANO;
	rtn
}

fn hpet_2_time(msg_header: &str, hpet: u64) -> u64 {
	let t = if cfg!(bm_in_us) {hpet_2_us(hpet)} else {hpet_2_ns(hpet)};
	if msg_header != "" {
		let mut msg = format!("{} {} in ", msg_header, t);
		msg += if cfg!(bm_in_us) {"us"} else {"ns"};
		printlninfo!("{}", msg);
	}

	t
}

// overhead is NOT time. It is COUNT
fn timing_overhead_inner(th: usize, nr: usize) -> u64 {
	let mut start_hpet_tmp: u64;
	let start_hpet: u64;
	let end_hpet: u64;

	// to warm cache and remove error
	start_hpet_tmp = get_hpet().as_ref().unwrap().get_counter();

	start_hpet = get_hpet().as_ref().unwrap().get_counter();
	for _ in 0..ITERATIONS {
		start_hpet_tmp = get_hpet().as_ref().unwrap().get_counter();
	}
	end_hpet = get_hpet().as_ref().unwrap().get_counter();

	let delta_hpet = end_hpet - start_hpet;
	let delta_hpet_avg = (end_hpet - start_hpet) / ITERATIONS as u64;

	printlninfo!("t_overhead_inner ({}/{}): {} total_ctr -> {} avg_ctr (ignore: {})", 
		th, nr, delta_hpet, delta_hpet_avg, start_hpet_tmp);
	delta_hpet_avg
}

// overhead is NOT time. It is COUNT
fn timing_overhead() -> u64 {
	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;

	// printlninfo!("Calculating timing_overhead. Patience...");
	for i in 0..TRIES {
		let overhead = timing_overhead_inner(i+1, TRIES);
		tries += overhead;
		if overhead > max {max = overhead;}
		if overhead < min {min = overhead;}
	}

	let overhead = tries / TRIES as u64;
	let err = (overhead * 10 + overhead * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - overhead > err || overhead - min > err {
		printlnwarn!("timing_overhead diff is too big: {} ({} - {}) ctr", max-min, max, min);
	}

	printlninfo!("Timing overhead: {} ctr\n\n", overhead);

	overhead
}

fn do_null_inner(overhead_ct: u64, th: usize, nr: usize) -> u64 {
	let start_hpet: u64;
	let end_hpet: u64;
	let mut mypid = core::usize::MAX;

	start_hpet = get_hpet().as_ref().unwrap().get_counter();
	for _ in 0..ITERATIONS {
		mypid = getpid();
	}
	end_hpet = get_hpet().as_ref().unwrap().get_counter();

	let mut delta_hpet: u64 = end_hpet - start_hpet;
	if delta_hpet < overhead_ct {
		printlnwarn!("Ignore overhead for null because overhead({}) > diff({})", overhead_ct, delta_hpet);
	} else {
		delta_hpet -= overhead_ct;
	}

	let delta_time = hpet_2_time("", delta_hpet);
	let delta_time_avg = delta_time / ITERATIONS as u64;

	printlninfo!("null_test_inner ({}/{}): {} total_time -> {} {} (ignore: {})", 
		th, nr, delta_time, delta_time_avg, T_UNIT, mypid);

	delta_time_avg
}

fn do_null() {
	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;

	let overhead_ct = timing_overhead();
	
	for i in 0..TRIES {
		let lat = do_null_inner(overhead_ct, i+1, TRIES);

		tries += lat;
		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("null_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	printlninfo!("NULL result: {} {}", lat, T_UNIT);
}

fn pick_child_core() -> Result<u8, &'static str> {
	// try with current core -1
	let child_core: u8 = CPU_ID!() as u8 - 1;
	if nr_tasks_in_rq(child_core) == Some(1) {return Ok(child_core);}

	// if failed, try from the last to the first
	for child_core in (0..apic::core_count() as u8).rev() {
		if nr_tasks_in_rq(child_core) == Some(1) {return Ok(child_core);}
	}

	Err("Cannot pick a core for children")
}

fn do_spawn_inner(overhead_ct: u64, th: usize, nr: usize, child_core: u8) -> Result<u64, &'static str> {
	use spawn::ApplicationTaskBuilder;
    let start_hpet: u64;
	let end_hpet: u64;

	start_hpet = get_hpet().as_ref().unwrap().get_counter();
	for _ in 0..ITERATIONS {
		let child = ApplicationTaskBuilder::new(Path::new(String::from("hello")))
	        .pin_on_core(child_core) // the child is always in the my core -1
	        .argument(Vec::new())
	        .spawn()?;

	    child.join().expect("Cannot join child");
	    child.take_exit_value().expect("Cannot take the exit value");
	}
    end_hpet = get_hpet().as_ref().unwrap().get_counter();

    let delta_hpet = end_hpet - start_hpet - overhead_ct;
    let delta_time = hpet_2_time("", delta_hpet);
    let delta_time_avg = delta_time / ITERATIONS as u64;
	printlninfo!("spawn_test_inner ({}/{}): : {} total_time -> {} {}", 
		th, nr, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}

fn do_spawn() {
	let child_core = match pick_child_core() {
		Ok(child_core) => { 
			printlninfo!("core_{} is idle, so my children will play on it.", child_core); 
			child_core
		}
		_ => {
			printlninfo!("Cannot conduct spawn test because cores are busy");
			return;
		}
	};

	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;

	let overhead_ct = timing_overhead();
	
	for i in 0..TRIES {
		let lat = do_spawn_inner(overhead_ct, i+1, TRIES, child_core).expect("Error in spawn inner()");

		tries += lat;
		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("spawn_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	printlninfo!("SPAWN result: {} {}", lat, T_UNIT);
}

fn get_cwd() -> Option<DirRef> {
	if let Some(taskref) = task::get_my_current_task() {
        let locked_task = &taskref.lock();
        let curr_env = locked_task.env.lock();
        return Some(Arc::clone(&curr_env.working_dir));
    }

    None
}

fn mk_tmp_file(filename: &str, sz: usize) -> Result<(), &'static str> {
	if let Some(fileref) = get_file(filename) {
		if fileref.lock().size() == sz {
			printlninfo!("{} exits", filename);
			return Ok(());
		}
	}

	let mut output = String::new();
	for i in 0..sz-1 {
		output.push((i as u8 % 10 + 48) as char);
	}
	output.push('!'); // my magic char for the last byte

    MemFile::new(filename.to_string(), output.as_bytes(), &get_cwd().unwrap()).expect("File cannot be created.");

	printlninfo!("{} is created.", filename);
	Ok(())
}

fn cat(fileref: &FileRef, sz: usize, msg: &str) {
	printlninfo!("{}", msg);
	let mut file = fileref.lock();
	let mut buf = vec![0 as u8; sz];

	match file.read(&mut buf) {
		Ok(nr_read) => {
			printlninfo!("tries to read {} bytes, and {} bytes are read", sz, nr_read);
			printlninfo!("read: '{}'", str::from_utf8(&buf).unwrap());
		}
		Err(_) => {printlninfo!("Cannot read");}
	}
}

fn write(fileref: &FileRef, sz: usize, msg: &str) {
	printlninfo!("{}", msg);
	let mut buf = vec![0 as u8; sz];

	for i in 0..sz {
		buf[i] = i as u8 % 10 + 48;
	}

	let mut file = fileref.lock();
	match file.write(&buf) {
		Ok(nr_write) => {
			printlninfo!("tries to write {} bytes, and {} bytes are written", sz, nr_write);
			printlninfo!("written: '{}'", str::from_utf8(&buf).unwrap());
		}
		Err(_) => {printlninfo!("Cannot write");}
	}
}

fn test_file_inner(fileref: FileRef) {
	let sz = {fileref.lock().size()};

	cat(&fileref, sz, 	"== Do CAT-NORMAL ==");
	cat(&fileref, sz*2,	"== Do CAT-MORE   ==");

	write(&fileref, sz, "== Do WRITE-NORMAL ==");
	cat(&fileref, sz, 	"== Do CAT-NORMAL ==");

	write(&fileref, sz*2, "== Do WRITE-MORE ==");
	let sz = {fileref.lock().size()};
	cat(&fileref, sz, 	"== Do CAT-NORMAL ==");

}

fn get_file(filename: &str) -> Option<FileRef> {
	let path = Path::new(filename.to_string());
	match path.get(&get_cwd().unwrap()) {
		Ok(file_dir_enum) => {
			match file_dir_enum {
                FileOrDir::File(fileref) => { Some(fileref) }
                _ => {None}
            }
		}
		_ => { None }
	}
}

fn test_file(filename: &str) {
	if let Some(fileref) = get_file(filename) {
		test_file_inner(fileref);
	}
}

fn do_fs() {
	let filename = format!("tmp{}.txt", getpid());
	if mk_tmp_file(&filename, 4).is_ok() {
		printlninfo!("Testing with the file...");
		test_file(&filename);
	}
}

fn do_fs_create_del() {
	printlninfo!("Cannot test without MemFile::Delete()...");
}

fn do_fs_read_with_open_inner(filename: &str, overhead_ct: u64, th: usize, nr: usize) -> Result<u64, &'static str> {
	let start_hpet: u64;
	let end_hpet: u64;
	let path = Path::new(filename.to_string());
	let mut dummy_sum: u64 = 0;
	let mut buf = vec![0; READ_BUF_SIZE];
	let mut unread_size = match get_file(filename) {
		Some(fileref) => {fileref.lock().size()}
		_ => {
			return Err("Cannot get the size");
		}
	} as i64;

	if unread_size % READ_BUF_SIZE as i64 != 0 {
		return Err("File size is not alligned");
	}

	start_hpet = get_hpet().as_ref().unwrap().get_counter();
	for _ in 0..ITERATIONS 	{
		let file_dir_enum = path.get(&get_cwd().unwrap()).expect("Cannot find file");
		match file_dir_enum {
            FileOrDir::File(fileref) => { 
            	let mut file = fileref.lock();	// so far, open()

            	while unread_size > 0 {	// now read()
                	// XXX: With the Current API, we cannot specify an offset. 
                	// But the API is coming soon. for now, pretend we have it
                	let nr_read = file.read(&mut buf).expect("Cannot read");
					unread_size -= nr_read as i64;
					dummy_sum += buf.iter().fold(0 as u64, |acc, &x| acc + x as u64);
            	}

            }
            _ => {
				return Err("dir or does not exist");
			}
        }
	}
	end_hpet = get_hpet().as_ref().unwrap().get_counter();

	let delta_hpet = end_hpet - start_hpet - overhead_ct;
	let delta_time = hpet_2_time("", delta_hpet);
	let delta_time_avg = delta_time / ITERATIONS as u64;
	printlninfo!("read_with_open_inner ({}/{}): : {} total_time -> {} {} (ignore: {})", 
		th, nr, delta_time, delta_time_avg, T_UNIT, dummy_sum);

	Ok(delta_time_avg)
}

fn do_fs_read_only_inner(filename: &str, overhead_ct: u64, th: usize, nr: usize) -> Result<u64, &'static str> {
	let start_hpet: u64;
	let end_hpet: u64;
	let path = Path::new(filename.to_string());
	let mut dummy_sum: u64 = 0;
	let mut buf = vec![0; READ_BUF_SIZE];
	let mut unread_size = match get_file(filename) {
		Some(fileref) => {fileref.lock().size()}
		_ => {
			return Err("Cannot get the size");
		}
	} as i64;

	if unread_size % READ_BUF_SIZE as i64 != 0 {
		return Err("File size is not alligned");
	}

	let file_dir_enum = path.get(&get_cwd().unwrap()).expect("Cannot find file");
	match file_dir_enum {
        FileOrDir::File(fileref) => { 
        	let mut file = fileref.lock();	// so far, open()

			start_hpet = get_hpet().as_ref().unwrap().get_counter();
			for _ in 0..ITERATIONS 	{
            	while unread_size > 0 {	// now read()
                	// XXX: With the Current API, we cannot specify an offset. 
                	// But the API is coming soon. for now, pretend we have it
                	let nr_read = file.read(&mut buf).expect("Cannot read");
					unread_size -= nr_read as i64;
					dummy_sum += buf.iter().fold(0 as u64, |acc, &x| acc + x as u64);
            	}
			}	// for
			end_hpet = get_hpet().as_ref().unwrap().get_counter();

        }
        _ => {
			return Err("dir or does not exist");
		}
    }

	let delta_hpet = end_hpet - start_hpet - overhead_ct;
	let delta_time = hpet_2_time("", delta_hpet);
	let delta_time_avg = delta_time / ITERATIONS as u64;
	printlninfo!("read_only_inner ({}/{}): : {} total_time -> {} {} (ignore: {})", 
		th, nr, delta_time, delta_time_avg, T_UNIT, dummy_sum);

	Ok(delta_time_avg)
}

fn do_fs_read_with_size(overhead_ct: u64, fsize_kb: usize, with_open: bool) {
	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;

	let filename = format!("tmp_{}k.txt", fsize_kb);
	mk_tmp_file(&filename, fsize_kb*1024).expect("Cannot create a file");

	for i in 0..TRIES {
		let lat = if with_open {
			do_fs_read_with_open_inner(&filename, overhead_ct, i+1, TRIES).expect("Error in read_open inner()")
		} else {
			do_fs_read_only_inner(&filename, overhead_ct, i+1, TRIES).expect("Error in read_only inner()")
		};

		tries += lat;
		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	let lat = tries / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	printlninfo!("{} for {} KB: {} {}", if with_open {"READ WITH OPEN"} else {"READ ONLY"}, fsize_kb, lat, T_UNIT);
}

fn do_fs_read(with_open: bool) {
	let overhead_ct = timing_overhead();

	// min: 64K
	for i in [64, 128, 256, 512, 1024].iter() {
        do_fs_read_with_size(overhead_ct, *i, with_open);
    }
}

fn nr_tasks_in_rq(core: u8) -> Option<usize> {
	match runqueue::RunQueue::get_runqueue(core).map(|rq| rq.read()) {
		Some(rq) => { Some(rq.iter().count()) }
		_ => { None }
	}
}

fn check_myrq() -> bool {
	match nr_tasks_in_rq(CPU_ID!()) {
		Some(2) => { true }
		_ => { false }
	}
}

fn print_usage(prog: &String) {
	printlninfo!("\nUsage: {} cmd", prog);
	printlninfo!("\n  availavle cmds:");
	printlninfo!("\n    null             : null syscall");
	printlninfo!("\n    spawn            : process creation");
	printlninfo!("\n    fs_read_with_open: file read including open");
	printlninfo!("\n    fs_read_only     : file read");
	printlninfo!("\n    fs_create        : file create + del");
}

fn print_header() {
	printlninfo!("========================================");
	printlninfo!("Time unit : {}", T_UNIT);
	printlninfo!("Iterations: {}", ITERATIONS);
	printlninfo!("Tries     : {}", TRIES);
	printlninfo!("Core      : {}", CPU_ID!());
	printlninfo!("========================================");
}

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
	let prog = get_prog_name();

    if args.len() != 1 {
    	print_usage(&prog);
    	return 0;
    }

    if !check_myrq() {
    	printlninfo!("{} cannot run on a busy core (#{}). Pin me on an idle core.", prog, CPU_ID!());
    	return 0;
    }

	print_header();
    
    match args[0].as_str() {
    	"null" => {
    		do_null();
    	}
    	"spawn" => {
    		do_spawn();
    	}
    	"fs_read_with_open" | "fs1" => {
    		do_fs_read(true /*with_open*/);
    	}
    	"fs_read_only" | "fs2" => {
    		do_fs_read(false /*with_open*/);
    	}
    	"fs_create" | "fs3" => {
    		do_fs_create_del();
    	}
    	"fs" => {	// test code for checking FS' ability
    		do_fs();
    	}
    	_arg => {
    		printlninfo!("Unknown command: {}", args[0]);
    		print_usage(&prog);
    		return 0;
    	}
    }
    
    0
}