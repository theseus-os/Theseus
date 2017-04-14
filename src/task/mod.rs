use core::sync::atomic::AtomicUsize;


// declare types "TaskId" as a usize and AtomicTaskId as an Atomic usize
int_like!(TaskId, usize);
int_like!(AtomicTaskId, AtomicUsize);



/// Used to ensure that context switches are done atomically
static CONTEXT_SWITCH_LOCK: AtomicBool = ATOMIC_BOOL_INIT;
pub static CURRENT_TASK: AtomicTaskId = AtomicTaskId::default();




pub fn init() {
    // TODO write init function for tasking
}




pub enum RunState = {
    RUNNING,
    PAUSED, 
    STOPPED,
    BLOCKED, 
}

struct Task {
    id: TaskId,
    runstate: RunState, 
    arch_state: ArchTaskState,
}


impl Task {

    fn new() -> Task { 
        Task {
            id: , // TODO FIXME add static id counter
            runstate: RunState::STOPPED, 
            arch_state: ArchTaskState::new()
        }
    }

    /// switches from the current (`self`)  to the `next` `Task`
    pub fn context_switch(&mut self, &mut next: &Task) {
        // Set the global lock to avoid the unsafe operations below from causing issues
        while CONTEXT_SWITCH_LOCK.compare_and_swap(false, true, Ordering::SeqCst) {
            arch::pause();
        }

        assert!(next.runstate != RunState::BLOCKED, "scheduler bug: chosen 'next' Task was BLOCKED!");
        assert!(next.runstate != RunState::RUNNING, "scheduler bug: chosen 'next' Task was already RUNNING!");

        // update runstates
        self.runstate = RunState::PAUSED; 
        next.runstate = RunState::RUNNING; 


        // store the current context ID
        CURRENT_TASK.store((next.id, Ordering::SeqCst);

        // FIXME: releasing the lock here is a temporary workaround, as there is only one CPU active right now
        arch::context::CONTEXT_SWITCH_LOCK.store(false, Ordering::SeqCst);


        // perform the actual context switch
        self.arch_state.switch_to(&next.arch_state);
    }


}