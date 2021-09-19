use core::cell::RefCell;
use cortex_m::interrupt::Mutex as ExcpCell;
use cortex_m::{self, interrupt, peripheral::syst::SystClkSource, Peripherals};
use memory::{allocate_pages_by_bytes, get_kernel_mmi_ref, EntryFlags};
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

    let stack_pages = allocate_pages_by_bytes(512).unwrap();
    let mapped_stack_pages = kernel_mmi
        .page_table
        .map_allocated_pages(stack_pages, EntryFlags::WRITABLE)
        .unwrap();
    let stack = Stack::from_pages(mapped_stack_pages).unwrap();

    let _bootstrap_task = spawn::init(0, stack).unwrap();

    spawn::create_idle_task(Some(0)).unwrap();

    let tb1 = new_task_builder(task_hello, 233);
    let _tr1 = tb1.spawn().unwrap();

    let tb2 = new_task_builder(task_world, 466);
    let _tr2 = tb2.spawn().unwrap();

    interrupt::free(|cs| {
        let mut p = PERIPHERALS.borrow(cs).borrow_mut();
        let syst = &mut p.SYST;

        // Configures the system timer to trigger a SysTick exception every 10ms.
        // It has a default CPU clock of 16 MHz so we set the counter to 160_000.
        syst.set_clock_source(SystClkSource::Core);
        syst.set_reload(160_000);
        syst.clear_current();
        syst.enable_counter();
        syst.enable_interrupt();
    });

    loop {}
}

fn task_hello(arg: usize) {
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
