use core::cell::RefCell;
use core::sync::atomic::AtomicUsize;
use cortex_m::interrupt::Mutex as ExcpCell;
use cortex_m::{self, interrupt, peripheral::syst::SystClkSource, Peripherals};
use memory::{allocate_pages_by_bytes_at, get_kernel_mmi_ref, EntryFlags};
use memory_structs::VirtualAddress;
use spawn::{self, new_task_builder};
use stack::Stack;

lazy_static! {
    /// A singleton structure representing all of the peripherals on the board.
    static ref PERIPHERALS: ExcpCell<RefCell<Peripherals>> = {
        let p = cortex_m::Peripherals::take().unwrap();
        ExcpCell::new(RefCell::new(p))
    };
}

pub fn init() -> ! {
    let kernel_mmi_ref = get_kernel_mmi_ref().unwrap();
    let mut kernel_mmi = kernel_mmi_ref.lock();

    // Allocate "pages" for the bootstrap task. Note that the position of these "pages"
    // should be placed at the end of the SRAM region, which is consistent with
    // kernellink.ld linker script. These "pages" are not real pages, but are only
    // 64-byte chunks as the unit of memory management. We are operating directly
    // on physical memory.
    let stack_pages =
        allocate_pages_by_bytes_at(VirtualAddress::new_canonical(0x2001_f800), 2048).unwrap();
    let mapped_stack_pages = kernel_mmi
        .page_table
        .map_allocated_pages(stack_pages, EntryFlags::WRITABLE)
        .unwrap();
    let stack = Stack::from_pages(mapped_stack_pages).unwrap();

    drop(kernel_mmi);
    drop(kernel_mmi_ref);

    // Initialize the bootstrap task. It represents the code currently running.
    // We only have one CPU, which is numbered 0.
    let bootstrap_task = spawn::init(0, stack).unwrap();

    // Create an idle task on CPU 0.
    spawn::create_idle_task(Some(0)).unwrap();

    cfg_if! {
        if #[cfg(realtime_scheduler)] {
            // build and spawn two real time tasks
            let tb3 = new_task_builder(task_delay_ten_seconds, 1, Some(1000));
            tb3.spawn().unwrap();
            let tb4 = new_task_builder(task_delay_two_seconds, 2, Some(200));
            tb4.spawn().unwrap();
        }
        else {
            // Build and spawn two tasks.
            let tb1 = new_task_builder(task_hello, 233);
            tb1.spawn().unwrap();
            let tb2 = new_task_builder(task_world, 466);
            tb2.spawn().unwrap();
        }
    }


    interrupt::free(move |cs| {
        let mut p = PERIPHERALS.borrow(cs).borrow_mut();
        let syst = &mut p.SYST;

        // Configures the system timer to trigger a SysTick exception every 10ms.
        // It has a default CPU clock of 16 MHz so we set the counter to 160_000.
        syst.set_clock_source(SystClkSource::Core);
        syst.set_reload(160_000);
        syst.clear_current();
        syst.enable_counter();
        syst.enable_interrupt();

        // Now that we've created a new idle task for this core, we can drop ourself's bootstrapped task.
        drop(bootstrap_task);
    });

    // ****************************************************
    // NOTE: nothing below here is guaranteed to run again!
    // ****************************************************
    scheduler::schedule();

    loop {
        error!("BUG: captain::init(): captain's bootstrap task was rescheduled after being dead!");
    }
}

fn task_hello(arg: usize) {
    use alloc::string::ToString;
    let arg = arg.to_string();
    loop {
        use cortex_m::asm;
        for _ in 0..100000000 {
            asm::nop();
        }
        info!("hello! arg: {}", arg);
    }
}

fn task_world(arg: usize) {
    loop {
        use cortex_m::asm;
        for _ in 0..100000000 {
            asm::nop();
        }
        info!("world! arg: {}", arg);
    }
}

fn task_delay_ten_seconds(arg: usize) {
    let start_time : AtomicUsize = AtomicUsize::new(interrupts::get_current_time_in_ticks());
    loop {
        info!("I run every ten seconds!");

        // Since we trigger a Tick every 10ms, 10 seconds will be 1000 ticks
        task_delay::delay_task_until(&start_time, 1000);
    }
}

fn task_delay_two_seconds(arg: usize) {
    let start_time : AtomicUsize = AtomicUsize::new(interrupts::get_current_time_in_ticks());
    loop {
        info!("I run every two seconds!");

        // Since we trigger a Tick every 10ms, 2 seconds will be 200 ticks
        task_delay::delay_task_until(&start_time, 200);
    }
}