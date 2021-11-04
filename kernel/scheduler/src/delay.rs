use core::sync::atomic::{Ordering, AtomicUsize};
use priority_queue::priority_queue::PriorityQueue;
use hashbrown::hash_map::DefaultHashBuilder;
use irq_safety::MutexIrqSafe;
use task::get_my_current_task;

lazy_static! {
	/// List of all delayed tasks in the system
	/// Implemented as a priority queue where the key is the unblocking time and the value is the id of the task
	static ref DELAYED_TASKLIST: MutexIrqSafe<PriorityQueue<usize, usize, DefaultHashBuilder>> = MutexIrqSafe::new(PriorityQueue::with_default_hasher());
}

/// Keeps track of the next task that needs to unblock, by default, it is the maximum time
pub static NEXT_DELAYED_TASK_UNBLOCK_TIME : AtomicUsize = AtomicUsize::new(usize::MAX);


/// Helper function adds the id associated with a TaskRef to the list of delayed tasks with priority equal to the time when the task must resume work
/// If the resume time is less than the current earliest resume time, we will update it
fn add_to_delayed_tasklist(taskid: usize, resume_time: usize) {
	DELAYED_TASKLIST.lock().push(taskid, resume_time);
	
	let next_unblock_time = NEXT_DELAYED_TASK_UNBLOCK_TIME.load(Ordering::SeqCst);
	if (resume_time < next_unblock_time) {
		NEXT_DELAYED_TASK_UNBLOCK_TIME.store(resume_time, Ordering::SeqCst);
	}
}

/// Remove the next task from the delayed task list and unblock that task
pub fn remove_next_task_from_delayed_tasklist() {
	let mut delayed_tasklist = DELAYED_TASKLIST.lock();
	if let Some((taskid, resume_time)) = delayed_tasklist.pop() {
	if let Some(task) = task::TASKLIST.lock().get(&taskid) {
		task.unblock();
	}

	match delayed_tasklist.peek() {
		Some((new_taskid, new_resume_time)) => NEXT_DELAYED_TASK_UNBLOCK_TIME.store(*new_resume_time, Ordering::SeqCst),
		None => NEXT_DELAYED_TASK_UNBLOCK_TIME.store(usize::MAX, Ordering::SeqCst),
	}
	}
}

/// Delay the current task for a fixed time period after the time in ticks specified by last_resume_time
/// Then we will update last_resume_time to its new value by adding the time period to its old value
pub fn delay_task_until(last_resume_time: & AtomicUsize, period_length: usize) {
	let new_resume_time = last_resume_time.fetch_add(period_length, Ordering::SeqCst) + period_length;

	if let Some(current_task) = get_my_current_task() {
		/// block current task and add it to the delayed tasklist
		current_task.block();
		let taskid = current_task.lock().id;
		add_to_delayed_tasklist(taskid, new_resume_time);
		super::schedule();
	}
	
}
