#![feature(restricted_std, unboxed_closures)]

fn temp() {
    println!("Hello from temp");
}

pub fn main() {
    let p: Box<dyn FnOnce<(), Output = ()>> = Box::new(temp);
    let mmi_ref = memory::get_kernel_mmi_ref().unwrap();
    // let stack = stack::alloc_stack_by_bytes(4096, &mut mmi_ref.lock().page_table).unwrap();
    let stack = stack::alloc_stack_by_bytes(4096 * 16, &mut mmi_ref.lock().page_table).unwrap();
    println!("{:#?}", stack);
    let thread = spawn::new_task_builder(|_| p(), ())
        .stack(stack)
        .spawn()
        .unwrap();
    thread.join().unwrap();

    // println!("Hello, world!");

    // std::thread::spawn(|| {
    //     println!("Printing from thread 2");
    // })
    // .join()
    // .unwrap();
}
