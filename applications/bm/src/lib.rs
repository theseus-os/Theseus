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
extern crate scheduler;

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
const MICRO_TO_FEMTO: u64 = 1_000_000_000;
const NANO_TO_FEMTO: u64 = 1_000_000;
const SEC_TO_NANO: u64 = 1_000_000_000;
const SEC_TO_MICRO: u64 = 1_000_000;
// const MB_IN_KB: usize = 1024;
const MB: u64 = 1024 * 1024;
const KB: u64 = 1024;

// for testing..
const ITERATIONS: usize = 1_000;
const TRIES: usize = 10;

// don't change it. 
const READ_BUF_SIZE: usize = 64*1024;
const WRITE_BUF_SIZE: usize = 1024*1024;
const WRITE_BUF: [u8; WRITE_BUF_SIZE] = [65; WRITE_BUF_SIZE];

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
	hpet * hpet_period as u64 / MICRO_TO_FEMTO
}

fn hpet_2_ns(hpet: u64) -> u64 {
	let hpet_period = get_hpet().as_ref().unwrap().counter_period_femtoseconds();
	hpet * hpet_period as u64 / NANO_TO_FEMTO
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

fn print_stats(vec: Vec<u64>) {
	let avg;
  	let median;
  	let perf_75;
	let perf_25;
	let min;
	let max;
	let var;

  	{ // calculate average
      let mut sum: u64 = 0;
      for x in &vec {
          sum = sum + x;
      }

      avg = sum as u64 / vec.len() as u64;
  	}

	{ // calculate median
	  let mut vec2 = vec.clone();
      vec2.sort();
      let mid = vec2.len() / 2;
	  let p_75 = vec2.len() *3 / 4;
	  let p_25 = vec2.len() *1 / 4;

      median = vec2[mid];
	  perf_25 = vec2[p_25];
	  perf_75 = vec2[p_75];
	  min = vec2[0];
	  max = vec2[vec.len() - 1];
  	}

	{
		//calculate standard deviation
		let mut sum: u64 = 0;
		let mut sq_sum: u64 = 0;
      	for x in &vec {
			sq_sum = sq_sum + x*x;
      	}

    	var = ((sq_sum as u64) - (vec.len() as u64)*(avg)*(avg)) / (vec.len() as u64 - 1);

	}
	printlninfo!("\n  mean : {}",avg);
	printlninfo!("\n  var  : {}",var);
	printlninfo!("\n  max  : {}",max);
	printlninfo!("\n  min  : {}",min);
	printlninfo!("\n  p_50 : {}",median);
	printlninfo!("\n  p_25 : {}",perf_25);
	printlninfo!("\n  p_75 : {}",perf_75);
	printlninfo!("\n");
}

fn do_null_inner(overhead_ct: u64, th: usize, nr: usize) -> u64 {
	let start_hpet: u64;
	let end_hpet: u64;
	let mut mypid = core::usize::MAX;

	let tmp_iterations = ITERATIONS *1000;
	start_hpet = get_hpet().as_ref().unwrap().get_counter();
	for _ in 0..tmp_iterations {
		mypid = task::get_my_current_task_id().unwrap();
	}
	end_hpet = get_hpet().as_ref().unwrap().get_counter();

	let mut delta_hpet: u64 = end_hpet - start_hpet;
	if delta_hpet < overhead_ct {
		printlnwarn!("Ignore overhead for null because overhead({}) > diff({})", overhead_ct, delta_hpet);
	} else {
		delta_hpet -= overhead_ct;
	}

	let delta_time = hpet_2_time("", delta_hpet);
	let delta_time_avg = delta_time / ((ITERATIONS*1000) as u64);

	printlninfo!("null_test_inner ({}/{}): hpet {} , overhead {}, {} total_time -> {} {} (ignore: {})",
		th, nr, delta_hpet, overhead_ct, delta_time, delta_time_avg, T_UNIT, mypid);

	delta_time_avg
}

fn do_null() {
	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::new();

	let overhead_ct = timing_overhead();
	
	for i in 0..TRIES {
		let lat = do_null_inner(overhead_ct, i+1, TRIES);

		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	print_stats(vec);
	let lat = tries / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("null_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	printlninfo!("NULL result: {} {}", lat, T_UNIT);
	printlninfo!("This test is equivalent to `lat_syscall null` in LMBench");
}

fn pick_child_core() -> Result<u8, &'static str> {
	// try with current core -1
	let child_core: u8 = CPU_ID!() as u8 - 1;
	// return Ok(child_core); //Comment for real hardware debugging
	if nr_tasks_in_rq(child_core) == Some(1) {return Ok(child_core);}

	// if failed, try from the last to the first
	for child_core in (0..apic::core_count() as u8).rev() {
		if nr_tasks_in_rq(child_core) == Some(1) {return Ok(child_core);}
	}
	printlninfo!("Cannot pick a child core because cores are busy");
	return Ok(child_core);
}

fn do_spawn_inner(overhead_ct: u64, th: usize, nr: usize, child_core: u8) -> Result<u64, &'static str> {
	use spawn::ApplicationTaskBuilder;
    let start_hpet: u64;
	let end_hpet: u64;
	let tmp_iterations: u64 = 100;
	start_hpet = get_hpet().as_ref().unwrap().get_counter();
	for _ in 0..tmp_iterations {
		let child = ApplicationTaskBuilder::new(Path::new(String::from("hello")))
	        .pin_on_core(child_core) // the child is always in the my core -1
	        //.argument(Vec::new())
	        .spawn()?;

	    child.join().expect("Cannot join child");
	    child.take_exit_value().expect("Cannot take the exit value");
	}
    end_hpet = get_hpet().as_ref().unwrap().get_counter();

    let delta_hpet = end_hpet - start_hpet - overhead_ct;
    let delta_time = hpet_2_time("", delta_hpet);
    let delta_time_avg = delta_time / tmp_iterations as u64;
	printlninfo!("spawn_test_inner ({}/{}): hpet {} , overhead {}, {} total_time -> {} {}",
		th, nr, delta_hpet, overhead_ct, delta_time, delta_time_avg, T_UNIT);

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
	let mut vec = Vec::new();

	let overhead_ct = timing_overhead();
	
	for i in 0..TRIES {
		let lat = do_spawn_inner(overhead_ct, i+1, TRIES, child_core).expect("Error in spawn inner()");

		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	print_stats(vec);

	let lat = tries / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("spawn_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	printlninfo!("SPAWN result: {} {}", lat, T_UNIT);
	printlninfo!("This test is equivalent to `lat_proc exec` in LMBench");
}

fn do_ctx_inner(th: usize, nr: usize, child_core: u8) -> Result<u64, &'static str> {
	use spawn::KernelTaskBuilder;
    let start_hpet: u64;
	let end_hpet: u64;
	let overhead_end_hpet: u64;

	// we first span two tasks to get the overhead

	start_hpet = get_hpet().as_ref().unwrap().get_counter();

		let taskref3 = KernelTaskBuilder::new(overhead_task ,1)
        .name(String::from("overhead_task"))
        .pin_on_core(child_core)
        .spawn().expect("failed to initiate task");

		let taskref4 = KernelTaskBuilder::new(overhead_task ,2)
			.name(String::from("overhead_task"))
			.pin_on_core(child_core)
			.spawn().expect("failed to initiate task");

		taskref3.join().expect("Task 1 join failed");
		taskref4.join().expect("Task 2 join failed");

	overhead_end_hpet = get_hpet().as_ref().unwrap().get_counter();

	// we then span them with yielding enabled

		let taskref1 = KernelTaskBuilder::new(yield_task ,1)
        .name(String::from("yield_task"))
        .pin_on_core(child_core)
        .spawn().expect("failed to initiate task");

		let taskref2 = KernelTaskBuilder::new(yield_task ,2)
			.name(String::from("yield_task"))
			.pin_on_core(child_core)
			.spawn().expect("failed to initiate task");

		taskref1.join().expect("Task 1 join failed");
		taskref2.join().expect("Task 2 join failed");

    end_hpet = get_hpet().as_ref().unwrap().get_counter();

    let delta_overhead = overhead_end_hpet - start_hpet;
	let delta_hpet = end_hpet - overhead_end_hpet - delta_overhead;
    let delta_time = hpet_2_time("", delta_hpet);
	let overhead_time = hpet_2_time("", delta_overhead);
    let delta_time_avg = delta_time / (ITERATIONS*1000*2) as u64; //*2 because each thread yields ITERATION number of times
	printlninfo!("ctx_switch_test_inner ({}/{}): total_overhead -> {} {} , {} total_time -> {} {}",
		th, nr, overhead_time, T_UNIT, delta_time, delta_time_avg, T_UNIT);

	Ok(delta_time_avg)
}

fn do_ctx() {
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

	// let child_core: u8 = CPU_ID!() as u8 - 1;

	let mut tries: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let mut vec = Vec::new();

	// let overhead_ct = timing_overhead(); // timing overhead is already calculated within inner
	
	for i in 0..TRIES {
		let lat = do_ctx_inner(i+1, TRIES, child_core).expect("Error in ctx inner()");
	
		tries += lat;
		vec.push(lat);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	print_stats(vec);

	let lat = tries / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("ctx_test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	printlninfo!("Context switch result: {} {}", lat, T_UNIT);
	// printlninfo!("This test is equivalent to `lat_proc exec` in LMBench");
}

fn get_cwd() -> Option<DirRef> {
	if let Some(taskref) = task::get_my_current_task() {
        let locked_task = &taskref.lock();
        let curr_env = locked_task.env.lock();
        return Some(Arc::clone(&curr_env.working_dir));
    }

    None
}

// Don't call this function inside of a measuring loop.
fn mk_tmp_file(filename: &str, sz: usize) -> Result<(), &'static str> {
	if sz > WRITE_BUF_SIZE {
		return Err("Cannot test because the file size is too big");
	}

	if let Some(fileref) = get_file(filename) {
		if fileref.lock().size() == sz {
			return Ok(());
		}
	}

	let file = MemFile::new(filename.to_string(), &get_cwd().unwrap()).expect("File cannot be created.");
	file.lock().write(&WRITE_BUF[0..sz], 0)?;

	Ok(())
}

fn del_or_err(filename: &str) -> Result<(), &'static str> {
	if let Some(_fileref) = get_file(filename) {
		return Err("Need to delete a file, but delete() is not implemented yet :(");
	}
	Ok(())
}

fn do_fs_create_del_inner(fsize_b: usize, overhead_ct: u64) -> Result<(), &'static str> {
	let mut filenames = vec!["".to_string(); ITERATIONS];
	let pid = getpid();
	let start_hpet_create: u64;
	let end_hpet_create: u64;
	let start_hpet_del: u64;
	let end_hpet_del: u64;

	// don't put these (populating files, checks, etc) into the loop to be timed
	// The loop must be doing minimal operations to exclude unnecessary overhead
	// populate filenames
	for i in 0..ITERATIONS {
		filenames[i] = format!("tmp_{}_{}_{}.txt", pid, fsize_b, i);
	}

	// check if we have enough data to write. We use just const data to avoid unnecessary overhead
	if fsize_b > WRITE_BUF_SIZE {
		return Err("Cannot test because the file size is too big");
	}

	// delete existing files. To make sure that the file creation below succeeds.
	for filename in &filenames {
		del_or_err(filename).expect("Cannot continue the test. We need 'delete()'.");
	}

	let cwd = match get_cwd() {
		Some(dirref) => {dirref}
		_ => {return Err("Cannot get CWD");}
	};

	let wbuf = &WRITE_BUF[0..fsize_b];

	// Measuring loop - create
	start_hpet_create = get_hpet().as_ref().unwrap().get_counter();
	for filename in &filenames {
		// checking if filename exists is done above
		// here, we only create files

		// We can create a file from mapped pages using 'from_mapped_pages(),'
		// but we first create a file and then write to resemble LMBench.
		let file = MemFile::new(filename.to_string(), &cwd).expect("File cannot be created.");
		file.lock().write(wbuf, 0)?;
	}
	end_hpet_create = get_hpet().as_ref().unwrap().get_counter();

	// // Measuring loop - delete
	// let mut cwd_locked = cwd.lock();
	// start_hpet_del = get_hpet().as_ref().unwrap().get_counter();

	// for filename in filenames {
	// 	if let Some(fileref) = get_file(&filename) {
	// 		cwd_locked.remove(&FileOrDir::File(fileref)).expect("Cannot remove File in Create & Del inner");
	// 	}
	// }

	// end_hpet_del = get_hpet().as_ref().unwrap().get_counter();

	let delta_hpet_create = end_hpet_create - start_hpet_create - overhead_ct;
	// let delta_hpet_delete = end_hpet_del - start_hpet_del - overhead_ct;
	let delta_time_create = hpet_2_time("", delta_hpet_create);
	// let delta_time_delete = hpet_2_time("", delta_hpet_delete);
	let to_sec: u64 = if cfg!(bm_in_us) {SEC_TO_MICRO} else {SEC_TO_NANO};
	let files_per_time = (ITERATIONS) as u64 * to_sec / delta_time_create;
	// let deletes_per_time = (ITERATIONS) as u64 * to_sec / delta_time_delete;

	printlninfo!("{:8}    {:9}    {:16}", fsize_b/KB as usize, ITERATIONS, files_per_time);
	Ok(())
}

fn cat(fileref: &FileRef, sz: usize, msg: &str) {
	printlninfo!("{}", msg);
	let file = fileref.lock();
	let mut buf = vec![0 as u8; sz];

	match file.read(&mut buf,0) {
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
	match file.write(&buf,0) {
		Ok(nr_write) => {
			printlninfo!("tries to write {} bytes, and {} bytes are written", sz, nr_write);
			printlninfo!("written: '{}'", str::from_utf8(&buf).unwrap());
		}
		Err(_) => {printlninfo!("Cannot write");}
	}
}

fn test_file_inner(fileref: FileRef) {
	let sz = {fileref.lock().size()};
	printlninfo!("File size: {}", sz);

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

// the function is not used now. but it can be used in the future, e.g., remove
fn get_file_in(filename: String, dirref: &DirRef) -> Option<FileRef> {
	let path = Path::new(filename.to_string());
	match path.get(dirref) {
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

fn do_fs_cap_check() {
	let filename = format!("tmp{}.txt", getpid());
	if mk_tmp_file(&filename, 4).is_ok() {
		printlninfo!("Testing with the file...");
		test_file(&filename);
	}
}

fn do_fs_create_del() {
	// let	fsizes_b = [0 as usize, 1024, 4096, 10*1024];	// Theseus thinks creating an empty file is stupid (for memfs)
	let	fsizes_b = [1024_usize, 4096, 10*1024];
	// let	fsizes_b = [1024_usize];

	let overhead_ct = timing_overhead();

	// printlninfo!("SIZE(KB)    Iteration    created(files/s)    deleted(files/s)");
	printlninfo!("SIZE(KB)    Iteration    created(files/s) ");
	// for i in 0..TRIES {
		for fsize_b in fsizes_b.iter() {
			do_fs_create_del_inner(*fsize_b, overhead_ct).expect("Cannot test File Create & Del");
		}
	//}
}

fn do_fs_read_with_open_inner(filename: &str, overhead_ct: u64, th: usize, nr: usize) -> Result<(u64, u64, u64), &'static str> {
	let start_hpet: u64;
	let end_hpet: u64;
	let path = Path::new(filename.to_string());
	let mut dummy_sum: u64 = 0;
	let mut buf = vec![0; READ_BUF_SIZE];
	let size = match get_file(filename) {
		Some(fileref) => {fileref.lock().size()}
		_ => {
			return Err("Cannot get the size");
		}
	} as i64;
	let mut unread_size = size;

	if unread_size % READ_BUF_SIZE as i64 != 0 {
		return Err("File size is not alligned");
	}

	start_hpet = get_hpet().as_ref().unwrap().get_counter();
	for _ in 0..ITERATIONS 	{
		let file_dir_enum = path.get(&get_cwd().unwrap()).expect("Cannot find file");
		match file_dir_enum {
            FileOrDir::File(fileref) => { 
            	let mut file = fileref.lock();	// so far, open()

            	unread_size = size;
            	while unread_size > 0 {	// now read()
                	// XXX: With the Current API, we cannot specify an offset. 
                	// But the API is coming soon. for now, pretend we have it
                	let nr_read = file.read(&mut buf,0).expect("Cannot read");
					unread_size -= nr_read as i64;

					// LMbench based on C does the magic to cast a type from char to int
					// But, we dont' have the luxury with type-safe Rust, so we do...
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

	let to_sec: u64 = if cfg!(bm_in_us) {SEC_TO_MICRO} else {SEC_TO_NANO};
	let mb_per_sec = (size as u64 * to_sec) / (MB * delta_time_avg);	// prefer this
	let kb_per_sec = (size as u64 * to_sec) / (KB * delta_time_avg);

	printlninfo!("read_with_open_inner ({}/{}): {} total_time -> {} {} {} MB/sec {} KB/sec (ignore: {})",
		th, nr, delta_time, delta_time_avg, T_UNIT, mb_per_sec, kb_per_sec, dummy_sum);

	Ok((delta_time_avg, mb_per_sec, kb_per_sec))
}

fn do_fs_read_only_inner(filename: &str, overhead_ct: u64, th: usize, nr: usize) -> Result<(u64, u64, u64), &'static str> {
	let start_hpet: u64;
	let end_hpet: u64;
	let path = Path::new(filename.to_string());
	let mut dummy_sum: u64 = 0;
	let mut buf = vec![0; READ_BUF_SIZE];
	let size = match get_file(filename) {
		Some(fileref) => {fileref.lock().size()}
		_ => {
			return Err("Cannot get the size");
		}
	} as i64;
	let mut unread_size = size;

	if unread_size % READ_BUF_SIZE as i64 != 0 {
		return Err("File size is not alligned");
	}

	let file_dir_enum = path.get(&get_cwd().unwrap()).expect("Cannot find file");
	match file_dir_enum {
        FileOrDir::File(fileref) => { 
        	let mut file = fileref.lock();	// so far, open()

			start_hpet = get_hpet().as_ref().unwrap().get_counter();
			for _ in 0..ITERATIONS 	{
				unread_size = size;
            	while unread_size > 0 {	// now read()
                	// XXX: With the Current API, we cannot specify an offset. 
                	// But the API is coming soon. for now, pretend we have it
                	let nr_read = file.read(&mut buf,0).expect("Cannot read");
					unread_size -= nr_read as i64;

					// LMbench based on C does the magic to cast a type from char to int
					// But, we dont' have the luxury with type-safe Rust, so we do...
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

	let to_sec: u64 = if cfg!(bm_in_us) {SEC_TO_MICRO} else {SEC_TO_NANO};
	let mb_per_sec = (size as u64 * to_sec) / (MB * delta_time_avg);	// prefer this
	let kb_per_sec = (size as u64 * to_sec) / (KB * delta_time_avg);

	printlninfo!("read_only_inner ({}/{}): {} total_time -> {} {} {} MB/sec {} KB/sec (ignore: {})",
		th, nr, delta_time, delta_time_avg, T_UNIT, mb_per_sec, kb_per_sec, dummy_sum);

	Ok((delta_time_avg, mb_per_sec, kb_per_sec))
}

fn do_fs_read_with_size(overhead_ct: u64, fsize_kb: usize, with_open: bool) {
	let mut tries: u64 = 0;
	let mut tries_mb: u64 = 0;
	let mut tries_kb: u64 = 0;
	let mut max: u64 = core::u64::MIN;
	let mut min: u64 = core::u64::MAX;
	let fsize_b = fsize_kb * KB as usize;
	let mut vec = Vec::new();

	let filename = format!("tmp_{}k.txt", fsize_kb);

	// we can use `mk_tmp_file()` because it is outside of the loop
	mk_tmp_file(&filename, fsize_b).expect("Cannot create a file");

	for i in 0..TRIES {
		let (lat, tput_mb, tput_kb) = if with_open {
			do_fs_read_with_open_inner(&filename, overhead_ct, i+1, TRIES).expect("Error in read_open inner()")
		} else {
			do_fs_read_only_inner(&filename, overhead_ct, i+1, TRIES).expect("Error in read_only inner()")
		};

		tries += lat;
		tries_mb += tput_mb;
		tries_kb += tput_kb;
		vec.push(tput_kb);

		if lat > max {max = lat;}
		if lat < min {min = lat;}
	}

	print_stats(vec);

	let lat = tries / TRIES as u64;
	let tput_mb = tries_mb / TRIES as u64;
	let tput_kb = tries_kb / TRIES as u64;
	let err = (lat * 10 + lat * THRESHOLD_ERROR_RATIO) / 10;
	if 	max - lat > err || lat - min > err {
		printlnwarn!("test diff is too big: {} ({} - {}) {}", max-min, max, min, T_UNIT);
	}

	printlninfo!("{} for {} KB: {} {} {} MB/sec {} KB/sec", 
		if with_open {"READ WITH OPEN"} else {"READ ONLY"}, 
		fsize_kb, lat, T_UNIT, tput_mb, tput_kb);
}

fn do_fs_read(with_open: bool) {
	let fsize_kb = 1024;
	printlninfo!("File size     : {} KB", fsize_kb);
	printlninfo!("Read buf size : {} KB", READ_BUF_SIZE / 1024);
	printlninfo!("========================================");

	let overhead_ct = timing_overhead();

	do_fs_read_with_size(overhead_ct, fsize_kb, with_open);
	if with_open {
		printlninfo!("This test is equivalent to `bw_file_rd open2close` in LMBench");
	} else {
		printlninfo!("This test is equivalent to `bw_file_rd io_only` in LMBench");
	}
}

fn nr_tasks_in_rq(core: u8) -> Option<usize> {
	match runqueue::get_runqueue(core).map(|rq| rq.read()) {
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
	printlninfo!("\n    ctx        		 : inter thread context switching overhead");
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
		// return 0; //uncomment for real hardware debugging
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
		"ctx" => {
			do_ctx();
		}
		"fs" => {	// test code for checking FS' ability
			do_fs_cap_check();
		}
		_arg => {
			printlninfo!("Unknown command: {}", args[0]);
			print_usage(&prog);
			return 0;
		}
	}

	0
}

fn yield_task(_a: u32) -> u32 {
	let times = ITERATIONS*1000;
    for _i in 0..times {
       scheduler::schedule();
    }
    _a
}

fn overhead_task(_a: u32) -> u32 {
    _a
}