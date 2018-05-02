#![no_std]
#![feature(alloc)]
#![feature(asm, naked_functions)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate tss;
extern crate apic;


use core::fmt;
use core::sync::atomic::{Ordering, AtomicUsize, AtomicBool, spin_loop_hint};
use alloc::String;
use alloc::arc::Arc;

use irq_safety::{MutexIrqSafe, RwLockIrqSafe};
use memory::{PageTable, Stack, MemoryManagementInfo, VirtualAddress};
use atomic_linked_list::atomic_map::AtomicMap;
use apic::get_my_apic_id;
use tss::tss_set_rsp0;


/// The id of the currently executing `Task`, per-core.
lazy_static! {
    pub static ref CURRENT_TASKS: AtomicMap<u8, usize> = AtomicMap::new();
}

/// Used to ensure that context switches are done atomically on each core
lazy_static! {
    pub static ref CONTEXT_SWITCH_LOCKS: AtomicMap<u8, AtomicBool> = AtomicMap::new();
}

/// The list of all Tasks in the system.
lazy_static! {
    pub static ref TASKLIST: AtomicMap<usize, Arc<RwLockIrqSafe<Task>>> = AtomicMap::new();
}


/// Get the id of the currently running Task on a specific core
pub fn get_current_task_id(apic_id: u8) -> Option<usize> {
    CURRENT_TASKS.get(&apic_id).cloned()
}

/// Get the id of the currently running Task on this current task
pub fn get_my_current_task_id() -> Option<usize> {
    get_my_apic_id().and_then(|id| {
        get_current_task_id(id)
    })
}

/// returns a shared reference to the current `Task`
pub fn get_my_current_task() -> Option<&'static Arc<RwLockIrqSafe<Task>>> {
    get_my_current_task_id().and_then(|id| {
        TASKLIST.get(&id)
    })
}

/// returns a shared reference to the `Task` specified by the given `task_id`
pub fn get_task(task_id: usize) -> Option<&'static Arc<RwLockIrqSafe<Task>>> {
    TASKLIST.get(&task_id)
}




#[repr(u8)] // one byte
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum RunState {
    /// in the midst of setting up the task
    INITING = 0,
    /// able to be scheduled in, but not currently running
    RUNNABLE,
    /// blocked on something, like I/O or a wait event
    BLOCKED,
    /// thread has completed and is ready for cleanup
    EXITED,
}



pub struct Task {
    /// the unique id of this Task.
    pub id: usize,
    /// which cpu core the Task is currently running on.
    /// negative if not currently running.
    pub running_on_cpu: isize,
    /// the runnability status of this task, basically whether it's allowed to be scheduled in.
    pub runstate: RunState,
    /// the saved stack pointer value, used for context switching.
    pub saved_sp: usize,
    /// the simple name of this Task
    pub name: String,
    /// the kernelspace stack.  Wrapped in Option<> so we can initialize it to None.
    pub kstack: Option<Stack>,
    /// the userspace stack.  Wrapped in Option<> so we can initialize it to None.
    pub ustack: Option<Stack>,
    /// memory management details: page tables, mappings, allocators, etc.
    /// Wrapped in an Arc & Mutex because it's shared between all other tasks in the same address space
    pub mmi: Option<Arc<MutexIrqSafe<MemoryManagementInfo>>>, 
    /// for special behavior of new userspace task
    pub new_userspace_entry_addr: Option<VirtualAddress>, 
    /// Whether or not this task is pinned to a certain core
    /// The idle tasks (like idle_task) are always pinned to their respective cores
    pub pinned_core: Option<u8>,
    /// Whether this Task is an idle task, the task that runs by default when no other task is running.
    /// There exists one idle task per core.
    pub is_an_idle_task: bool,
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{Task \"{}\" ({}), running_on_cpu: {}, runstate: {:?}, pinned: {:?}}}", 
               self.name, self.id, self.running_on_cpu, self.runstate, self.pinned_core)
    }
}


/// The counter of task IDs
static TASKID_COUNTER: AtomicUsize = AtomicUsize::new(0);


impl Task {
    /// creates a new Task structure and initializes it to be non-Runnable.
    pub fn new() -> Task {
        // we should re-use old task IDs again, instead of simply blindly counting up
        let task_id = TASKID_COUNTER.fetch_add(1, Ordering::Acquire);
        
        Task {
            id: task_id,
            runstate: RunState::INITING,
            running_on_cpu: -1, // not running on any cpu
            saved_sp: 0,
            name: format!("task{}", task_id),
            kstack: None,
            ustack: None,
            mmi: None,
            new_userspace_entry_addr: None,
            pinned_core: None,
            is_an_idle_task: false,
        }
    }

    /// set the name of this Task
    pub fn set_name(&mut self, n: String) {
        self.name = n;
    }

    /// set the RunState of this Task
    pub fn set_runstate(&mut self, rs: RunState) {
        self.runstate = rs;
    }

    /// returns true if this Task is currently running on any cpu.
    pub fn is_running(&self) -> bool {
        self.running_on_cpu >= 0
    }

    pub fn is_runnable(&self) -> bool {
        self.runstate == RunState::RUNNABLE
    }

    pub fn is_an_idle_task(&self) -> bool {
        self.is_an_idle_task
    }


    /// switches from the current (`self`)  to the given `next` Task
    /// no locks need to be held to call this, but interrupts (later, preemption) should be disabled
    pub fn context_switch(&mut self, next: &mut Task, apic_id: u8) {
        // debug!("context_switch [0]: (AP {}) prev {:?}, next {:?}", apic_id, self, next);
        
        let my_context_switch_lock: &AtomicBool;
        if let Some(csl) = CONTEXT_SWITCH_LOCKS.get(&apic_id) {
            my_context_switch_lock = csl;
        } 
        else {
            error!("context_switch(): no context switch lock present for AP {}, skipping context switch!", apic_id);
            return;
        }
        
        // acquire this core's context switch lock
        // TODO: add timeout
        while my_context_switch_lock.compare_and_swap(false, true, Ordering::SeqCst) {
            spin_loop_hint();
        }

        // debug!("context_switch [1], testing runstates.");
        if next.runstate != RunState::RUNNABLE {
            error!("Skipping context_switch due to scheduler bug: chosen 'next' Task was not RUNNABLE! Current: {:?}, Next: {:?}", self, next);
            my_context_switch_lock.store(false, Ordering::SeqCst);
            return;
        }
        if next.running_on_cpu != -1 {
            error!("Skipping context_switch due to scheduler bug: chosen 'next' Task was already running on AP {}!\nCurrent: {:?} Next: {:?}", apic_id, self, next);
            my_context_switch_lock.store(false, Ordering::SeqCst);
            return;
        }
        if let Some(pc) = next.pinned_core {
            if pc != apic_id {
                error!("Skipping context_Switch due to scheduler bug: chosen 'next' Task was pinned to AP {:?} but scheduled on AP {}!\nCurrent: {:?}, Next: {:?}", next.pinned_core, apic_id, self, next);
                my_context_switch_lock.store(false, Ordering::SeqCst);
                return;
            }
        }
         

        // update runstates
        self.running_on_cpu = -1; // no longer running
        next.running_on_cpu = apic_id as isize; // now running on this core


        // change the privilege stack (RSP0) in the TSS
        // TODO: we can safely skip setting the TSS RSP0 when switching to kernel threads, i.e., when next is not a userspace task
        {
            let next_kstack = next.kstack.as_ref().expect("context_switch(): error: next task's kstack was None!");
            let new_tss_rsp0 = next_kstack.bottom() + (next_kstack.size() / 2); // the middle half of the stack
            if tss_set_rsp0(new_tss_rsp0).is_ok() { 
                // debug!("context_switch [2]: new_tss_rsp = {:#X}", new_tss_rsp0);
            }
            else {
                error!("context_switch(): failed to set AP {} TSS RSP0, aborting context switch!", apic_id);
                my_context_switch_lock.store(false, Ordering::SeqCst);
                return;
            }
        }

        // We now do the page table switching here, so we can use our higher-level PageTable abstractions
        {
            let prev_mmi = self.mmi.as_ref().expect("context_switch: couldn't get prev task's MMI!");
            let next_mmi = next.mmi.as_ref().expect("context_switch: couldn't get next task's MMI!");
            

            if Arc::ptr_eq(prev_mmi, next_mmi) {
                // do nothing because we're not changing address spaces
                // debug!("context_switch [3]: prev_mmi is the same as next_mmi!");
            }
            else {
                // time to change to a different address space and switch the page tables!

                let mut prev_mmi_locked = prev_mmi.lock();
                let mut next_mmi_locked = next_mmi.lock();
                // debug!("context_switch [3]: switching tables! From {} {:?} to {} {:?}", 
                //         self.name, prev_mmi_locked.page_table, next.name, next_mmi_locked.page_table);
                

                let new_active_table = {
                    // prev_table must be an ActivePageTable, and next_table must be an InactivePageTable
                    match &mut prev_mmi_locked.page_table {
                        &mut PageTable::Active(ref mut active_table) => {
                            active_table.switch(&next_mmi_locked.page_table)
                        }
                        _ => {
                            panic!("context_switch(): prev_table must be an ActivePageTable!");
                        }
                    }
                };
                
                // since we're no longer changing the prev page table to be inactive, just leave it be,
                // and only change the next task's page table to active 
                // (it was either active already, or it was previously inactive (and now active) if it was the first time it had been run)
                next_mmi_locked.set_page_table(PageTable::Active(new_active_table)); 

            }
        }
       
        // update the current task to `next`
        CURRENT_TASKS.insert(apic_id, next.id); 

        // release this core's context switch lock
        my_context_switch_lock.store(false, Ordering::SeqCst);

        unsafe {
            // debug!("context_switch [4]: prev sp: {:#X}, next sp: {:#X}", self.saved_sp, next.saved_sp);
            
            // because task_switch must be a naked function, we cannot directly pass it parameters
            // instead, we must pass our 2 parameters in RDI and RSI respectively
            asm!("mov rdi, $0; \
                  mov rsi, $1;" 
                : : "r"(&mut self.saved_sp as *mut usize), "r"(next.saved_sp)
                : "memory" : "intel", "volatile"
            );
            task_switch();
        }

    }
}

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{{{}}}", self.name, self.id)
    }
}



#[allow(private_no_mangle_fns)]
#[naked]
#[no_mangle]
#[inline(never)]
/// Performs the actual context switch from prev to next task.
/// First argument  (rdi): mutable pointer to the previous task's stack pointer
/// Second argument (rsi): the value of the next task's stack pointer
unsafe fn task_switch() {
    
    // this is the regular context switch for when x87 FPU/SSE is not enabled
    #[cfg(not(target_feature = "sse2"))]
    asm!("
        # save all general purpose registers into the previous task
        push rbx
        push rbp
        push r12
        push r13
        push r14
        push r15
        
        # switch the stack pointers
        mov [rdi], rsp
        mov rsp, rsi

        # restore the next task's general purpose registers
        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx

        # pops the last value off the top of the stack,
        # so the new task's stack top must point to a target function
        ret"
        : : : "memory" : "intel", "volatile"
    );



    // this is the context switch for when x87 FPU/SSE is enabled
    // we need to save xmm# registers in addition to the regular registers
    #[cfg(target_feature = "sse2")]
    asm!("
        # save all general purpose registers into the previous task
        push rbx
        push rbp
        push r12
        push r13
        push r14
        push r15
        
        # save all of the xmm registers (for SSE)
        # each register is 16 bytes, and there are 16 of them
        lea rsp, [rsp - 16*16]
        movdqu [rsp + 16*0],  xmm0   # push xmm0
        movdqu [rsp + 16*1],  xmm1   # push xmm1
        movdqu [rsp + 16*2],  xmm2   # push xmm2
        movdqu [rsp + 16*3],  xmm3   # push xmm3
        movdqu [rsp + 16*4],  xmm4   # push xmm4
        movdqu [rsp + 16*5],  xmm5   # push xmm5
        movdqu [rsp + 16*6],  xmm6   # push xmm6
        movdqu [rsp + 16*7],  xmm7   # push xmm7
        movdqu [rsp + 16*8],  xmm8   # push xmm8
        movdqu [rsp + 16*9],  xmm9   # push xmm9
        movdqu [rsp + 16*10], xmm10  # push xmm10
        movdqu [rsp + 16*11], xmm11  # push xmm11
        movdqu [rsp + 16*12], xmm12  # push xmm12
        movdqu [rsp + 16*13], xmm13  # push xmm13
        movdqu [rsp + 16*14], xmm14  # push xmm14
        movdqu [rsp + 16*15], xmm15  # push xmm15
        
        # switch the stack pointers
        mov [rdi], rsp
        mov rsp, rsi

        # restore all of the xmm registers
        movdqu xmm15, [rsp + 16*15]   # pop xmm15
        movdqu xmm14, [rsp + 16*14]   # pop xmm14
        movdqu xmm13, [rsp + 16*13]   # pop xmm13
        movdqu xmm12, [rsp + 16*12]   # pop xmm12
        movdqu xmm11, [rsp + 16*11]   # pop xmm11
        movdqu xmm10, [rsp + 16*10]   # pop xmm10
        movdqu xmm9,  [rsp + 16*9]    # pop xmm9
        movdqu xmm8,  [rsp + 16*8]    # pop xmm8
        movdqu xmm7,  [rsp + 16*7]    # pop xmm7
        movdqu xmm5,  [rsp + 16*5]    # pop xmm5
        movdqu xmm6,  [rsp + 16*6]    # pop xmm6
        movdqu xmm4,  [rsp + 16*4]    # pop xmm4
        movdqu xmm3,  [rsp + 16*3]    # pop xmm3
        movdqu xmm2,  [rsp + 16*2]    # pop xmm2
        movdqu xmm1,  [rsp + 16*1]    # pop xmm1
        movdqu xmm0,  [rsp + 16*0]    # pop xmm0
        lea rsp, [rsp + 16*16]

        # restore the next task's general purpose registers
        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx
        
        # pops the last value off the top of the stack,
        # so the new task's stack top must point to a target function
        ret"
        : : : "memory" : "intel", "volatile"
    );
    
}
