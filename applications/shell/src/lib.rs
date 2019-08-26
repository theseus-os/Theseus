//! Shell with event-driven architecture
//! Commands that can be run are the names of the crates in the applications directory
//! 
//! The shell has the following responsibilities: handles key events delivered from terminal, manages terminal display,
//! spawns and manages tasks, and records previously executed user commands.
//! 
//! Problem: Currently there's no upper bound to the user command line history.

#![no_std]
#![feature(slice_concat_ext)]
extern crate frame_buffer;
extern crate keycodes_ascii;
extern crate spin;
extern crate dfqueue;
extern crate mod_mgmt;
extern crate spawn;
extern crate task;
extern crate runqueue;
extern crate memory;
extern crate event_types; 
extern crate window_manager;
extern crate text_display;
extern crate path;
extern crate root;
extern crate scheduler;
extern crate stdio;
extern crate core_io;
extern crate app_io;
extern crate fs_node;
extern crate terminal_print;
extern crate print;
extern crate environment;
extern crate libterm;

#[macro_use] extern crate alloc;
#[macro_use] extern crate log;

use event_types::{Event};
use keycodes_ascii::{Keycode, KeyAction, KeyEvent};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spawn::{ApplicationTaskBuilder, KernelTaskBuilder};
use path::Path;
use task::{TaskRef, ExitValue, KillReason};
use libterm::Terminal;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use alloc::sync::Arc;
use spin::{Mutex, MutexGuard};
use environment::Environment;
use alloc::boxed::Box;
use core::mem;
use alloc::collections::BTreeMap;
use stdio::{Stdio, KeyEventQueue, KeyEventQueueReader, KeyEventQueueWriter,
            StdioReader, StdioWriter};
use core_io::Write;
use core::ops::Deref;
use app_io::{IoStreams, IoControlFlags};
use fs_node::FileOrDir;
use alloc::slice::SliceConcatExt;

pub const APPLICATIONS_NAMESPACE_PATH: &'static str = "/namespaces/default/applications";

/// The status of a job.
#[derive(PartialEq)]
enum JobStatus {
    /// Normal state. All the tasks in this job are either running or exited.
    Running,
    /// The job is suspended (but not killed), e.g. upon ctrl-Z.
    /// All the tasks in this job are either blocked or exited.
    Stopped
}

/// This structure is used by shell to track its spawned applications. Each successfully
/// evaluated command line will create a `Job`. Each job contains one or more tasks.
/// Tasks are stored in `tasks` in the same sequence as in the command line.
/// When pipe is used, the i-th job's `stdout` is directed to the (i+1)-th job's `stdin`.
/// `stderr` is always read by shell and currently cannot be redirected.
struct Job {
    /// References to the tasks that form this job. They are stored in th esame sequence as
    /// in the command line.
    tasks: Vec<TaskRef>,
    /// A copy of the task ids. Mainly for performance optimization. Stored in the same
    /// sequence as `tasks`.
    task_ids: Vec<usize>,
    /// Status of the job.
    status: JobStatus,
    /// The stdio queues between the running application and the shell, or between
    /// running applications if pipe is used. Assume there are N tasks, counting from 0
    /// to (N-1). `pipe_queues[0]` is the input for the 0-th task. `pipe_queues[N]` is the
    /// output from the last task to shell. Other queues are the pipes between two
    /// applications. Note that there are (N+1) queues in total.
    pipe_queues: Vec<Stdio>,
    /// The stderr queues between the running applications and the shell. Assume there are
    /// N tasks, counting from 0 to (N-1). All of these queues are sending byte streams from
    /// the applications to the shell, and currently cannot be redirected. Note that there
    /// are N queues in total.
    stderr_queues: Vec<Stdio>,
    /// The input writer of the job. It is the writer to `pipe_queues[0]`.
    stdin_writer: StdioWriter,
    /// The output reader of the job. It is the reader to `pipe_queues[N]`.
    stdout_reader: StdioReader,
    /// Command line that was used to create the job.
    cmd: String
}

/// A main function that spawns a new shell and waits for the shell loop to exit before returning an exit value
#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    // No actuall use. Just to placate the compiler.
    let _dummy = 0;

    let _task_ref = match KernelTaskBuilder::new(shell_loop, _dummy)
        .name("shell_loop".to_string())
        .spawn() {
        Ok(task_ref) => { task_ref }
        Err(err) => {
            error!("{}", err);
            error!("failed to spawn shell");
            return -1; 
        }
    };

    loop {
        // block this task, because it never needs to actually run again
        if let Some(my_task) = task::get_my_current_task() {
            my_task.block();
        }
    }
    // TODO FIXME: once join() doesn't cause interrupts to be disabled, we can use join again instead of the above loop
    // waits for the terminal loop to exit before exiting the main function
    // match term_task_ref.join() {
    //     Ok(_) => { }
    //     Err(err) => {error!("{}", err)}
    // }
}

/// Errors when attempting to invoke an application from the terminal. 
enum AppErr {
    /// The command does not match the name of any existing application in the 
    /// application namespace directory. 
    NotFound(String),
    /// The terminal could not find the application namespace due to a filesystem error. 
    NamespaceErr,
    /// The terminal could not spawn a new task to run the new application.
    /// Includes the String error returned from the task spawn function.
    SpawnErr(String)
}

struct Shell {
    /// Variable that stores the task id of any application manually spawned from the terminal
    jobs: BTreeMap<isize, Job>,
    /// Map task number to job number.
    task_to_job: BTreeMap<usize, isize>,
    /// Reader to the key event queue. Applications can take it.
    key_event_consumer: Arc<Mutex<Option<KeyEventQueueReader>>>,
    /// Writer to the key event queue.
    key_event_producer: KeyEventQueueWriter,
    /// Foreground job number.
    fg_job_num: Option<isize>,
    /// The string that stores the users keypresses after the prompt
    cmdline: String,
    /// This buffer stores characters before sending them to running application on `enter` key strike
    input_buffer: String,
    /// Variable that tracks how far left the cursor is from the maximum rightmost position (above)
    left_shift: usize,
    /// Vector that stores the history of commands that the user has entered
    command_history: Vec<String>,
    /// Variable used to track the net number of times the user has pressed up/down to cycle through the commands
    /// ex. if the user has pressed up twice and down once, then command shift = # ups - # downs = 1 (cannot be negative)
    history_index: usize,
    /// When someone enters some commands, but before pressing `enter` it presses `up` to see previous commands,
    /// we must push it to command_history. We don't want to push it twice.
    buffered_cmd_recorded: bool,
    /// The consumer to the terminal's print dfqueue
    print_consumer: DFQueueConsumer<Event>,
    /// The producer to the terminal's print dfqueue
    print_producer: DFQueueProducer<Event>,
    /// The terminal's current environment
    env: Arc<Mutex<Environment>>,
    /// the terminal that is bind with the shell instance
    terminal: Arc<Mutex<Terminal>>
}

impl Shell {
    /// Create a new shell. Must provide a terminal to bind with the new shell in the argument.
    fn new() -> Result<Shell, &'static str> {
        // initialize another dfqueue for the terminal object to handle printing from applications
        let terminal_print_dfq: DFQueue<Event>  = DFQueue::new();
        let print_consumer = terminal_print_dfq.into_consumer();
        let print_producer = print_consumer.obtain_producer();

        let key_event_queue: KeyEventQueue = KeyEventQueue::new();
        let key_event_producer = key_event_queue.get_writer();
        let key_event_consumer = key_event_queue.get_reader();

        // Sets up the kernel to print to this terminal instance
        print::set_default_print_output(print_producer.obtain_producer());

        let env = Environment {
            working_dir: Arc::clone(root::get_root()), 
        };

        let terminal = app_io::get_terminal_or_default()?;
        terminal.lock().initialize_screen()?;

        Ok(Shell {
            jobs: BTreeMap::new(),
            task_to_job: BTreeMap::new(),
            key_event_consumer: Arc::new(Mutex::new(Some(key_event_consumer))),
            key_event_producer,
            fg_job_num: None,
            cmdline: String::new(),
            input_buffer: String::new(),
            left_shift: 0,
            command_history: Vec::new(),
            history_index: 0,
            buffered_cmd_recorded: false,
            print_consumer,
            print_producer,
            env: Arc::new(Mutex::new(env)),
            terminal
        })
    }

    /// Insert a character to the command line buffer in the shell.
    /// The position to insert is determined by the internal maintained variable `left_shift`,
    /// which indicates the position counting from the end of the command line.
    /// `sync_terminal` indicates whether the terminal screen will be synchronically updated.
    fn insert_char_to_cmdline(&mut self, c: char, sync_terminal: bool) -> Result<(), &'static str> {
        let insert_idx = self.cmdline.len() - self.left_shift;
        self.cmdline.insert(insert_idx, c);
        if sync_terminal {
            self.terminal.lock().insert_char_to_screen(c, self.left_shift)?;
        }
        Ok(())
    }

    /// Remove a character from the command line buffer in the shell. If there is nothing to
    /// be removed, it does nothing and returns.
    /// The position to insert is determined by the internal maintained variable `left_shift`,
    /// which indicates the position counting from the end of the command line.
    /// `sync_terminal` indicates whether the terminal screen will be synchronically updated.
    fn remove_char_from_cmdline(&mut self, erase_left: bool, sync_terminal: bool) -> Result<(), &'static str> {
        let mut left_shift = self.left_shift;
        if erase_left { left_shift += 1; }
        if left_shift > self.cmdline.len() || left_shift == 0 { return Ok(()); }
        let erase_idx = self.cmdline.len() - left_shift;
        self.cmdline.remove(erase_idx);
        if sync_terminal {
            self.terminal.lock().remove_char_from_screen(left_shift)?;
        }
        Ok(())
    }

    /// Clear the command line buffer.
    /// `sync_terminal` indicates whether the terminal screen will be synchronically updated.
    fn clear_cmdline(&mut self, sync_terminal: bool) -> Result<(), &'static str> {
        if sync_terminal {
            for _i in 0..self.cmdline.len() {
                self.terminal.lock().remove_char_from_screen(1)?;
            }
        }
        self.cmdline.clear();
        self.left_shift = 0;
        Ok(())
    }

    /// Set the command line to be a specific string.
    /// `sync_terminal` indicates whether the terminal screen will be synchronically updated.
    fn set_cmdline(&mut self, s: String, sync_terminal: bool) -> Result<(), &'static str> {
        if !self.cmdline.is_empty() {
            self.clear_cmdline(sync_terminal)?;
        }
        self.cmdline = s.clone();
        self.left_shift = 0;
        if sync_terminal {
            self.terminal.lock().print_to_terminal(s);
        }
        Ok(())
    }

    /// Insert a character to the input buffer to the application.
    /// `sync_terminal` indicates whether the terminal screen will be synchronically updated.
    fn insert_char_to_input_buff(&mut self, c: char, sync_terminal: bool) -> Result<(), &'static str> {
        self.input_buffer.push(c);
        if sync_terminal {
            self.terminal.lock().insert_char_to_screen(c, 0)?;
        }
        Ok(())
    }

    /// Remove a character from the input buffer to the application.
    /// `sync_terminal` indicates whether the terminal screen will be synchronically updated.
    fn remove_char_from_input_buff(&mut self, sync_terminal: bool) -> Result<(), &'static str> {
        let popped = self.input_buffer.pop();
        if popped.is_some() && sync_terminal {
            self.terminal.lock().remove_char_from_screen(1)?;
        }
        Ok(())
    }

    /// Move the cursor to the very beginning of the input command line.
    fn move_cursor_leftmost(&mut self) {
        self.left_shift = self.cmdline.len();
    }

    /// Move the cursor to the very end of the input command line.
    fn move_cursor_rightmost(&mut self) {
        self.left_shift = 0;
    }

    /// Move the cursor a character left. If the cursor is already at the beginning of the command line,
    /// it simply returns.
    fn move_cursor_left(&mut self) {
        if self.left_shift < self.cmdline.len() {
            self.left_shift += 1;
        }
    }

    /// Move the cursor a character right. If the cursor is already at the end of the command line,
    /// it simply returns.
    fn move_cursor_right(&mut self) {
        if self.left_shift > 0 {
            self.left_shift -= 1;
        }
    }

    /// Roll to the next previous command. If there is no more previous commands, it does nothing.
    fn goto_previous_command(&mut self) -> Result<(), &'static str> {
        if self.history_index == self.command_history.len() {
            return Ok(());
        }
        if self.history_index == 0 {
            let previous_input = self.cmdline.clone();
            if !previous_input.is_empty() {
                self.command_history.push(previous_input);
                self.history_index += 1;
                self.buffered_cmd_recorded = true;
            }
        }
        self.history_index += 1;
        let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
        self.set_cmdline(selected_command, true)?;
        Ok(())
    }

    /// Roll to the next recent command. If it is already the most recent command, it does nothing.
    fn goto_next_command(&mut self) -> Result<(), &'static str> {
        if self.history_index == 0 {
            return Ok(());
        }
        if self.history_index == 1 {
            if self.buffered_cmd_recorded {
                // command_histroy has at least one element. safe to unwrap here.
                let selected_command = self.command_history.pop().unwrap();
                self.set_cmdline(selected_command, true)?;
                self.history_index -= 1;
                self.buffered_cmd_recorded = false;
                return Ok(());
            }
        }
        self.history_index -=1;
        if self.history_index == 0 {
            self.clear_cmdline(true)?;
            return Ok(());
        }
        let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
        self.set_cmdline(selected_command, true)?;
        Ok(())
    }

    fn handle_key_event(&mut self, keyevent: KeyEvent) -> Result<(), &'static str> {       
        // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
        if keyevent.action != KeyAction::Pressed {
            return Ok(()); 
        }

        // Ctrl+C signals the shell to exit the job
        if keyevent.modifiers.control && keyevent.keycode == Keycode::C {
            if let Some(ref fg_job_num) = self.fg_job_num {
                let task_refs = match self.jobs.get(fg_job_num) {
                    Some(job) => job.tasks.clone(), 
                    None => {
                        self.clear_cmdline(true)?;
                        self.input_buffer.clear();
                        self.terminal.lock().print_to_terminal("^C\n".to_string());
                        self.redisplay_prompt();
                        return Ok(());
                    }
                };

                // Lock the shared structure in `app_io` and then kill the running application
                app_io::lock_and_execute(Box::new(move |_flags_guard: MutexGuard<BTreeMap<usize, IoControlFlags>>,
                                                        _streamss_guard: MutexGuard<BTreeMap<usize, IoStreams>>| {

                    // Kill all tasks in the job.
                    for task_ref in &task_refs {
                        if task_ref.lock().has_exited() { continue; }
                        match task_ref.kill(KillReason::Requested) {
                            Ok(_) => {
                                if let Err(e) = runqueue::remove_task_from_all(&task_ref) {
                                    error!("Killed task but could not remove it from runqueue: {}", e);
                                }
                            }
                            Err(e) => error!("Could not kill task, error: {}", e),
                        }

                        // Here we must wait for the running application to quit before releasing the lock,
                        // because the previous `kill` method will NOT stop the application immediately.
                        // Rather, it would let the application finish the current time slice before actually
                        // killing it.
                        loop {
                            scheduler::schedule(); // yield the CPU
                            if !task_ref.lock().is_running() {
                                break;
                            }
                        }
                    }
                }));
            } else {
                self.clear_cmdline(true)?;
                self.input_buffer.clear();
                self.terminal.lock().print_to_terminal("^C\n".to_string());
                self.redisplay_prompt();
            }
            return Ok(());
        }

        // Ctrl+Z signals the shell to stop the job
        if keyevent.modifiers.control && keyevent.keycode == Keycode::Z {
            // Do nothing if we have no running foreground job.

            if let Some(ref fg_job_num) = self.fg_job_num {
                let task_refs = match self.jobs.get(fg_job_num) {
                    Some(job) => job.tasks.clone(), 
                    None => {
                        return Ok(());
                    }
                };

                // Lock the shared structure in `app_io` and then stop the running application
                app_io::lock_and_execute(Box::new(move |_flags_guard: MutexGuard<BTreeMap<usize, IoControlFlags>>,
                                                        _streams_guard: MutexGuard<BTreeMap<usize, IoStreams>>| {
                    
                    // Stop all tasks in the job.
                    for task_ref in &task_refs {
                        if task_ref.lock().has_exited() { continue; }
                        task_ref.block();

                        // Here we must wait for the running application to stop before releasing the lock,
                        // because the previous `block` method will NOT stop the application immediately.
                        // Rather, it would let the application finish the current time slice before actually
                        // stopping it.
                        loop {
                            scheduler::schedule(); // yield the CPU
                            if !task_ref.lock().is_running() {
                                break;
                            }
                        }
                    }
                }));
            }

            return Ok(());
        }

        // Set EOF to the stdin of the foreground job.
        if keyevent.modifiers.control && keyevent.keycode == Keycode::D {
            if let Some(ref fg_job_num) = self.fg_job_num {
                if let Some(job) = self.jobs.get(fg_job_num) {
                    job.stdin_writer.lock().set_eof();
                }
            }
            return Ok(());
        }

        // Perform command line auto completion.
        if keyevent.keycode == Keycode::Tab {
            if self.fg_job_num.is_none() {
                self.complete_cmdline()?;
            }
            return Ok(());
        }

        // Tracks what the user does whenever she presses the backspace button
        if keyevent.keycode == Keycode::Backspace  {
            if self.fg_job_num.is_some() {
                self.remove_char_from_input_buff(true)?;
            } else {
                self.remove_char_from_cmdline(true, true)?;
            }
            return Ok(());
        }

        if keyevent.keycode == Keycode::Delete {
            self.remove_char_from_cmdline(false, true)?;
            return Ok(());
        }

        // Attempts to run the command whenever the user presses enter and updates the cursor tracking variables 
        if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            let cmdline = self.cmdline.clone();
            if cmdline.len() == 0 && self.fg_job_num.is_none() {
                // reprints the prompt on the next line if the user presses enter and hasn't typed anything into the prompt
                self.terminal.lock().print_to_terminal("\n".to_string());
                self.redisplay_prompt();
                return Ok(());
            } else if let Some(ref fg_job_num) = self.fg_job_num { // send buffered characters to the running application
                match self.jobs.get(fg_job_num) {
                    Some(job) => {
                        self.terminal.lock().print_to_terminal("\n".to_string());
                        let mut buffered_string = String::new();
                        mem::swap(&mut buffered_string, &mut self.input_buffer);
                        buffered_string.push('\n');
                        job.stdin_writer.lock().write_all(buffered_string.as_bytes())
                            .or(Err("shell failed to write to stdin"))?;
                    },
                    _ => {}
                }
                return Ok(());
            } else { // start a new job
                self.terminal.lock().print_to_terminal("\n".to_string());
                self.command_history.push(cmdline.clone());
                self.command_history.dedup(); // Removes any duplicates
                self.history_index = 0;

                if self.is_internal_command() { // shell executes internal commands
                    self.execute_internal()?;
                    self.clear_cmdline(false)?;
                    self.terminal.lock().refresh_display(0);
                } else { // shell invokes user programs
                    let new_job_num = self.build_new_job()?;
                    self.fg_job_num = Some(new_job_num);

                    // If the new job is to run in the background, then we should not put it to foreground.
                    if let Some(last) = self.cmdline.split_whitespace().last() {
                        if last == "&" {
                            self.terminal.lock().print_to_terminal(
                                format!("[{}] [running] {}\n", new_job_num, self.cmdline)
                                .to_string()
                            );
                            self.fg_job_num = None;
                            self.clear_cmdline(false)?;
                            self.redisplay_prompt();
                        }
                    }
                }
            }
            // Clears the buffer for next command once current command starts executing
            self.clear_cmdline(false)?;
            return Ok(());
        }

        // home, end, page up, page down, up arrow, down arrow for the input_event_manager
        if keyevent.keycode == Keycode::Home && keyevent.modifiers.control {
            self.terminal.lock().move_screen_to_begin();
            return Ok(());
        }
        if keyevent.keycode == Keycode::End && keyevent.modifiers.control{
            self.terminal.lock().move_screen_to_end();
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up  {
            self.terminal.lock().move_screen_line_up();
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down  {
            self.terminal.lock().move_screen_line_down();
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageUp && keyevent.modifiers.shift {
            self.terminal.lock().move_screen_page_up();
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageDown && keyevent.modifiers.shift {
            self.terminal.lock().move_screen_page_down();
            return Ok(());
        }

        // Cycles to the next previous command
        if  keyevent.keycode == Keycode::Up {
            self.goto_previous_command()?;
            return Ok(());
        }

        // Cycles to the next most recent command
        if keyevent.keycode == Keycode::Down {
            self.goto_next_command()?;
            return Ok(());
        }

        // Jumps to the beginning of the input string
        if keyevent.keycode == Keycode::Home {
            self.move_cursor_leftmost();
            return Ok(());
        }

        // Jumps to the end of the input string
        if keyevent.keycode == Keycode::End {
            self.move_cursor_rightmost();
            return Ok(());
        }

        // Adjusts the cursor tracking variables when the user presses the left and right arrow keys
        if keyevent.keycode == Keycode::Left {
            self.move_cursor_left();
            return Ok(());
        }

        if keyevent.keycode == Keycode::Right {
            self.move_cursor_right();
            return Ok(());
        }

        // Tracks what the user has typed so far, excluding any keypresses by the backspace and Enter key, which are special and are handled directly below
        if keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            match keyevent.keycode.to_ascii(keyevent.modifiers) {
                Some(c) => {
                    // If currently we have a task running, insert it to the input buffer, otherwise
                    // to the cmdline.
                    if let Some(fg_job_num) = self.fg_job_num {
                        self.insert_char_to_input_buff(c, true)?;
                        if let Some(job) = self.jobs.get(&fg_job_num) {
                            if app_io::is_requesting_instant_flush(&job.task_ids[0])? {
                                job.stdin_writer.lock().write_all(self.input_buffer.as_bytes())
                                    .or(Err("shell failed to write to stdin"))?;
                                self.input_buffer.clear();
                            }
                        }
                        return Ok(());
                    }
                    else {
                        self.insert_char_to_cmdline(c, true)?;
                    }
                },
                None => {
                    return Err("Couldn't get key event");
                }
            }
        }
        Ok(())
    }

    /// Create a single task. `cmd` is the name of the application. `args` are the provided
    /// argumeents. It returns a task reference on success.
    fn create_single_task(&mut self, cmd: String, args: Vec<String>) -> Result<TaskRef, AppErr> {

        // Check that the application actually exists
        let namespace_dir = task::get_my_current_task()
            .map(|t| t.get_namespace().dir().clone())
            .ok_or(AppErr::NamespaceErr)?;
        let cmd_crate_name = format!("{}-", cmd);
        let mut matching_apps = namespace_dir.get_files_starting_with(&cmd_crate_name).into_iter();
        let app_file = matching_apps.next();
        let second_match = matching_apps.next(); // return an error if there are multiple matching apps 
        let app_path = app_file.xor(second_match)
            .map(|f| Path::new(f.lock().get_absolute_path()))
            .ok_or(AppErr::NotFound(cmd))?;

        let taskref = match ApplicationTaskBuilder::new(app_path)
            .argument(args)
            .spawn(true) {
                Ok(taskref) => taskref, 
                Err(e) => return Err(AppErr::SpawnErr(e.to_string()))
            };
        
        taskref.set_env(self.env.clone()); // Set environment variable of application to the same as terminal task

        // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
        return Ok(taskref);
    }

    /// Evaluate the command line. It creates a sequence of jobs, which forms a chain of applications that
    /// pipe the output from one to the next, and finally back to the shell. If any task fails to start up,
    /// all tasks that have already been spawned will be killed immeidately before returning error.
    fn eval_cmdline(&mut self) -> Result<Vec<TaskRef>, AppErr> {

        let cmdline = self.cmdline.clone();
        let mut task_refs = Vec::new();

        for single_task_cmd in cmdline.split("|") {
            let mut args: Vec<String> = single_task_cmd.split_whitespace().map(|s| s.to_string()).collect();
            let command = args.remove(0);

            // If the last arg is `&`, remove it.
            if let Some(last_arg) = args.last() {
                if last_arg == "&" {
                    args.pop();
                }
            }
            match self.create_single_task(command, args) {
                Ok(task_ref) => task_refs.push(task_ref),

                // Once we run into an error, we must kill all previously spawned tasks in this command line.
                Err(e) => {
                    for task_ref in task_refs {
                        if let Err(kill_error) = task_ref.kill(KillReason::BrokenPipeAtSpawn) {
                            error!("{}", kill_error);
                        }
                    }
                    return Err(e);
                }
            }
        }
        Ok(task_refs)
    }

    /// Start a new job in the shell by the command line.
    fn build_new_job(&mut self) -> Result<isize, &'static str> {
        match self.eval_cmdline() {
            Ok(task_refs) => {

                let mut task_ids = Vec::new();
                let mut pipe_queues = Vec::new();
                let mut stderr_queues = Vec::new();

                for task_ref in &task_refs {
                    task_ids.push(task_ref.lock().id);
                }

                // Set up the chain of queues between applications, and between shell and application.
                // See the comments for `Job` to get a view of how queues are chained.
                let first_stdio_queue = Stdio::new();
                let job_stdin_writer = first_stdio_queue.get_writer();
                let mut previous_queue_reader = first_stdio_queue.get_reader();
                pipe_queues.push(first_stdio_queue);
                for task_id in &task_ids {
                    let stdio_queue_for_stdin_and_stdout = Stdio::new();
                    let stdio_queue_for_stderr = Stdio::new();
                    let terminal = app_io::get_terminal_or_default()?;
                    let streams = IoStreams::new(
                        previous_queue_reader,
                        stdio_queue_for_stdin_and_stdout.get_writer(),
                        stdio_queue_for_stderr.get_writer(),
                        self.key_event_consumer.clone(),
                        terminal
                    );
                    app_io::insert_child_streams(*task_id, streams);

                    previous_queue_reader = stdio_queue_for_stdin_and_stdout.get_reader();
                    stderr_queues.push(stdio_queue_for_stderr);
                    pipe_queues.push(stdio_queue_for_stdin_and_stdout);

                    // Insert print event producer to `terminal_print` to support legacy output.
                    if let Err(msg) = terminal_print::add_child(*task_id, self.print_producer.obtain_producer()) {
                        self.terminal.lock().print_to_terminal(format!("{}\n", msg).to_string());
                        return Err(msg);
                    }
                }

                let job_stdout_reader = previous_queue_reader;

                let new_job = Job {
                    tasks: task_refs,
                    task_ids,
                    status: JobStatus::Running,
                    pipe_queues,
                    stderr_queues,
                    stdin_writer: job_stdin_writer,
                    stdout_reader: job_stdout_reader,
                    cmd: self.cmdline.clone()
                };

                // All IO streams have been set up for the new tasks. Safe to unblock them now.
                for task_ref in &new_job.tasks {
                    task_ref.unblock();
                }

                // Allocate a job number for the new job. It will start from 1 and choose the smallest number
                // that has not yet been allocated.
                let mut new_job_num: isize = 1;
                for (key, _) in self.jobs.iter() {
                    if new_job_num != *key {
                        break;
                    }
                    new_job_num += 1;
                }

                // Map all tasks in the same job to the same job number.
                for task_id in &new_job.task_ids {
                    self.task_to_job.insert(*task_id, new_job_num);
                }

                self.jobs.insert(new_job_num, new_job);
                Ok(new_job_num)
            },
            Err(err) => {
                let err_msg = match err {
                    AppErr::NotFound(command) => format!("{:?} command not found.\n", command),
                    AppErr::NamespaceErr      => format!("Failed to find directory of application executables.\n"),
                    AppErr::SpawnErr(e)       => format!("Failed to spawn new task to run command. Error: {}.\n", e),
                };
                self.terminal.lock().print_to_terminal(err_msg);
                if let Err(msg) = self.clear_cmdline(false) {
                    self.terminal.lock().print_to_terminal(format!("{}\n", msg).to_string());
                }
                self.redisplay_prompt();
                Err("Failed to evaluate command line.")
            }
        }
    }

    /// Try to match the incomplete command against all internal commands. Returns a
    /// vector that contains all matching results.
    fn find_internal_cmd_match(&mut self, incomplete_cmd: &String) -> Result<Vec<String>, &'static str> {
        let internal_cmds = vec!["fg", "bg", "jobs", "clear"];
        let mut match_cmds = Vec::new();
        for cmd in internal_cmds.iter() {
            if cmd.starts_with(incomplete_cmd) {
                match_cmds.push(cmd.to_string());
            }
        }
        Ok(match_cmds)
    }

    /// Try to match the incomplete command against all applications in the same namespace.
    /// Returns a vector that contains all matching results.
    fn find_app_name_match(&mut self, incomplete_cmd: &String) -> Result<Vec<String>, &'static str> {
        let namespace_dir = match task::get_my_current_task()
            .map(|t| t.get_namespace().dir().clone())
            .ok_or(AppErr::NamespaceErr) {
            Ok(dir) => dir,
            Err(_) => {
                return Err("Failed to get namespace_dir while completing cmdline.");
            }
        };
        let cmd_crate_name = format!("{}", incomplete_cmd);
        Ok(namespace_dir.get_file_names_starting_with(&cmd_crate_name))
    }

    /// Try to match the incomplete command against all possible path names. For example, if the
    /// current command is `foo/bar/examp`, it first tries to walk the directory of `foo/bar`. If
    /// it succeeds, it then lists all filenames under `foo/bar` and tries to match `examp` against
    /// those filenames. It returns a vector that contains all matching results.
    fn find_file_path_match(&mut self, incomplete_cmd: &String) -> Result<Vec<String>, &'static str> {

        // Stores all possible matches.
        let mut match_list = Vec::new();

        let taskref = match task::get_my_current_task() {
            Some(t) => t,
            None => return Err("Failed to get task reference while completing cmdline.")
        };

        // Get current working dir.
        let mut curr_wd = {
            let locked_task = taskref.lock();
            let curr_env = locked_task.env.lock();
            Arc::clone(&curr_env.working_dir)
        };

        // Check if the last character is a slash.
        let slash_ending = match incomplete_cmd.chars().last() {
            Some('/') => true,
            _ => false
        };

        // Split the path by slash and filter out consecutive slashes.
        let mut nodes: Vec<_> = incomplete_cmd.split("/").filter(|node| { node.len() > 0 }).collect();

        // Get the last node in the path, which is to be completed.
        let incomplete_node = {
            // If the command ends with a slash, then we should list all files under
            // that directory. An empty string is always the prefix of any string.
            if slash_ending {
                ""
            } else {
                match nodes.pop() {
                    Some(node) => node,
                    None => return Ok(match_list)
                }
            }
        };

        // Walk through nodes existing in the command.
        for node in &nodes {
            let path = Path::new(node.to_string());
            match path.get(&curr_wd) {
                Some(file_dir_enum) => {
                    match file_dir_enum {
                        FileOrDir::Dir(dir) => { curr_wd = dir; },
                        FileOrDir::File(_file) => { return Ok(match_list); }
                    }
                },
                _ => { return Ok(match_list); }
            };
        }

        // Try to match the name of the file.
        let mut child_list = curr_wd.lock().list(); 
        child_list.reverse();
        for child in child_list.iter() {
            if child.starts_with(&incomplete_node) {
                match_list.push(child.clone());
            }
        }

        // Change the matching name in the list to full path names.
        let mut parent_dir = nodes.join("/");
        if !parent_dir.is_empty() { parent_dir.push('/'); }
        let mut full_path;
        for match_name in match_list.iter_mut() {
            full_path = parent_dir.clone() + match_name;
            core::mem::swap(&mut full_path, match_name);
        }
        Ok(match_list)
    }

    /// Automatically complete the half-entered command line if possible.
    /// If there exists only one possibility, the half-entered command line is completed.
    /// If there are several possibilities, it will show all possibilities.
    /// Otherwise, it does nothing.
    fn complete_cmdline(&mut self) -> Result<(), &'static str> {
        if self.cmdline.is_empty() {
            return Ok(());
        }

        // Get the last string slice in the pipe chain.
        let last_cmd_in_pipe = match self.cmdline.split("|").last() {
            Some(cmd) => cmd,
            None => return Ok(())
        };

        // Get the last word in the args (or maybe the command name itself).
        let last_word_in_cmd = match last_cmd_in_pipe.split(" ").last() {
            Some(word) => word.to_string(),
            None => return Ok(())
        };

        // Try to find matches.
        let mut possible_names = self.find_file_path_match(&last_word_in_cmd)?;
        possible_names.extend(self.find_internal_cmd_match(&last_word_in_cmd)?.iter().cloned());
        possible_names.extend(self.find_app_name_match(&last_word_in_cmd)?.iter().cloned());

        if !possible_names.is_empty() {

            // drop the extension name and hash value
            for name in &mut possible_names {
                let mut clean_name = String::new();
                if let Some(prefix) = name.split("-").next() {
                    clean_name = prefix.to_string();
                }
                mem::swap(name, &mut clean_name);
            }

            // only one possible name, complete the command line
            if possible_names.len() == 1 {
                for _ in 0..last_word_in_cmd.len() {
                    self.remove_char_from_cmdline(true, true)?;
                }
                for c in possible_names[0].chars() {
                    self.insert_char_to_cmdline(c, true)?;
                }
            } else { // several possible names, list them sequentially
                self.terminal.lock().print_to_terminal("\n".to_string());
                let mut is_first = true;
                for name in possible_names {
                    if !is_first {
                        self.terminal.lock().print_to_terminal("    ".to_string());
                    }
                    if let Some(basename) = name.split("/").last() {
                        self.terminal.lock().print_to_terminal(basename.to_string());
                    }
                    is_first = false;
                }
                self.terminal.lock().print_to_terminal("\n".to_string());
                self.redisplay_prompt();
            }
        }
        Ok(())
    }

    fn task_handler(&mut self) -> Result<(bool, bool), &'static str> {
        let mut need_refresh = false;
        let mut need_prompt = false;
        let mut job_to_be_removed: Vec<isize> = Vec::new();

        // Iterate through all jobs. If any job has exited, remove its stdio queues and remove it from
        // the job list. If any job has just stopped, mark it as stopped in the job list.
        for (job_num, job) in self.jobs.iter_mut() {
            let mut has_alive = false;  // mark if there is still non-exited task in the job
            let mut is_stopped = false; // mark if any one of the task has been stopped in this job

            let task_refs = job.tasks.clone();
            for task_ref in task_refs {
                if task_ref.lock().has_exited() { // a task has exited
                    let exited_task_id = task_ref.lock().id;
                    if let Some(exit_val) = task_ref.take_exit_value() {
                        match exit_val {
                            ExitValue::Completed(exit_status) => {
                                // here: the task ran to completion successfully, so it has an exit value.
                                // we know the return type of this task is `isize`,
                                // so we need to downcast it from Any to isize.
                                let val: Option<&isize> = exit_status.downcast_ref::<isize>();
                                info!("terminal: task [{}] returned exit value: {:?}", exited_task_id, val);
                                if let Some(val) = val {
                                    if *val < 0 {
                                        self.terminal.lock().print_to_terminal(
                                            format!("task [{}] returned error value {:?}\n", exited_task_id, val)
                                        );
                                    }
                                }
                            },

                            ExitValue::Killed(KillReason::Requested) => {
                                self.terminal.lock().print_to_terminal("^C\n".to_string());
                            },
                            // If the user manually aborts the task
                            ExitValue::Killed(kill_reason) => {
                                warn!("task [{}] was killed because {:?}", exited_task_id, kill_reason);
                                self.terminal.lock().print_to_terminal(
                                    format!("task [{}] was killed because {:?}\n", exited_task_id, kill_reason)
                                );
                            }
                        }

                        // Remove the queue for legacy terminal print
                        terminal_print::remove_child(exited_task_id)?;

                        need_refresh = true;

                        // Set EOF flag for the stdin, stdout of the exited task.
                        let mut pipe_queue_iter = job.pipe_queues.iter();
                        let mut stderr_queue_iter = job.stderr_queues.iter();
                        let mut task_id_iter = job.task_ids.iter();
                        while let (Some(pipe_queue), Some(stderr_queue), Some(task_id))
                            = (pipe_queue_iter.next(), stderr_queue_iter.next(), task_id_iter.next()) {

                            // Find the exited task by matching task id.
                            if *task_id == exited_task_id {

                                // Set the EOF flag of its `stdin`, which effectively prevents it's
                                // producer from writing more. (It returns an error upon writing to
                                // the queue which has the EOF flag set.)
                                pipe_queue.get_writer().lock().set_eof();

                                // Also set the EOF of `stderr`.
                                stderr_queue.get_writer().lock().set_eof();

                                // Set the EOF flag of its `stdout`, which effectively notifies the reader
                                // of the queue that the stream has ended. The `if let` clause should not
                                // fail.
                                if let Some(pipe_queue) = pipe_queue_iter.next() {
                                    pipe_queue.get_writer().lock().set_eof();
                                }
                                break;
                            }
                        }
                    }

                } else if !task_ref.lock().is_runnable() && job.status != JobStatus::Stopped { // task has just stopped

                    // One task in this job is stopped, but the status of the Job has not been set to
                    // `Stopped`. Let's set it now.
                    job.status = JobStatus::Stopped;

                    // If this is the foreground job, remove it from foreground.
                    if Some(*job_num) == self.fg_job_num {
                        self.fg_job_num = None;
                        need_prompt = true;
                    }

                    need_refresh = true;
                    has_alive = true;  // This task is stopped, but yet alive.
                    is_stopped = true; // Mark that this task is just stopped.
                } else {
                    has_alive = true;  // This is a running task, which is alive.
                }
            }

            // If the job is stopped (e.g. by ctrl-Z), print an noticifation to the terminal.
            if is_stopped {
                self.terminal.lock().print_to_terminal(
                    format!("[{}] [stopped] {}\n", job_num, job.cmd)
                    .to_string()
                );
            }

            // Record the completed job and remove them from job list later if all tasks in the
            // job have exited.
            if !has_alive {
                job_to_be_removed.push(*job_num);
                if self.fg_job_num == Some(*job_num) {
                    self.fg_job_num = None;
                    need_prompt = true;
                } else {
                    self.terminal.lock().print_to_terminal(
                        format!("[{}] [finished] {}\n", job_num, job.cmd)
                        .to_string()
                    );
                }
            }
        }

        // Print all remaining output for exited tasks.
        if self.check_and_print_app_output() {
            need_refresh = true;
        }

        // Trash all remaining unread keyboard events.
        if let Some(ref consumer) = *self.key_event_consumer.lock() {
            while let Some(_key_event) = consumer.read_one() {}
        }

        // Actually remove the exited jobs from the job list. We could not do it previously since we were
        // iterating through the job list. At the same time, remove them from the task_to_job mapping, and
        // remove the queues in app_io.
        for finished_job_num in job_to_be_removed {
            if let Some(job) = self.jobs.remove(&finished_job_num) {
                for task_id in job.task_ids {
                    self.task_to_job.remove(&task_id);
                    app_io::remove_child_streams(&task_id);
                }
            }
        }

        Ok((need_refresh, need_prompt))
    }

    /// Redisplays the terminal prompt (does not insert a newline before it)
    fn redisplay_prompt(&mut self) {
        let curr_env = self.env.lock();
        let mut prompt = curr_env.working_dir.lock().get_absolute_path();
        prompt = format!("{}: ",prompt);
        self.terminal.lock().print_to_terminal(prompt);
        self.terminal.lock().print_to_terminal(self.cmdline.clone());
    }

    /// If there is any output event from running application, print it to the screen, otherwise it does nothing.
    fn check_and_print_app_output(&mut self) -> bool {
        let mut need_refresh = false;

        // Support for legacy output by `terminal_print`.
        if let Some(print_event) = self.print_consumer.peek() {
            match print_event.deref() {
                &Event::OutputEvent(ref s) => {
                    self.terminal.lock().print_to_terminal(s.text.clone());

                    // Sets this bool to true so that on the next iteration the TextDisplay will refresh AFTER the 
                    // task_handler() function has cleaned up, which does its own printing to the console
                    self.terminal.lock().refresh_display(0);
                },
                _ => { },
            }
            print_event.mark_completed();
            // Goes to the next iteration of the loop after processing print event to ensure that printing is handled before keypresses
            need_refresh =  true;
        }

        let mut buf: [u8; 256] = [0; 256];

        // iterate through all jobs to see if they have something to print
        for (_job_num, job) in self.jobs.iter() {

            // Deal with all stdout output.
            let mut stdout = job.stdout_reader.lock();
            match stdout.try_read(&mut buf) {
                Ok(cnt) => {
                    mem::drop(stdout);
                    let s = String::from_utf8_lossy(&buf[0..cnt]);
                    let mut locked_terminal = self.terminal.lock();
                    locked_terminal.print_to_terminal(s.to_string());
                    if cnt != 0 { need_refresh = true; }
                },
                Err(_) => {
                    mem::drop(stdout);
                    error!("failed to read from stdout");
                }
            };

            // Deal with all stderr output.
            for stderr in &job.stderr_queues {
                let stderr = stderr.get_reader();
                let mut stderr = stderr.lock();
                match stderr.try_read(&mut buf) {
                    Ok(cnt) => {
                        mem::drop(stderr);
                        let s = String::from_utf8_lossy(&buf[0..cnt]);
                        let mut locked_terminal = self.terminal.lock();
                        locked_terminal.print_to_terminal(s.to_string());
                        if cnt != 0 { need_refresh = true; }
                    },
                    Err(_) => {
                        mem::drop(stderr);
                        error!("failed to read from stderr");
                    }
                };
            }
        }

        need_refresh
    }

    /// This main loop is the core component of the shell's event-driven architecture. The shell receives events
    /// from two queues
    /// 
    /// 1) The print queue handles print events from applications. The producer to this queue
    ///    is any EXTERNAL application that prints to the terminal.
    /// 
    /// 2) The input queue (provided by the window manager when the temrinal request a window) gives key events
    ///    and resize event to the application.
    /// 
    /// The print queue is handled first inside the loop iteration, which means that all print events in the print
    /// queue will always be printed to the text display before input events or any other managerial functions are handled. 
    /// This allows for clean appending to the scrollback buffer and prevents interleaving of text.
    fn start(mut self) -> Result<(), &'static str> {
        let mut need_refresh = false;
        let mut need_prompt = false;
        self.redisplay_prompt();
        self.terminal.lock().refresh_display(0);
        loop {
            self.terminal.lock().blink_cursor();

            // If there is anything from running applications to be printed, it printed on the screen and then
            // return true, so that the loop continues, otherwise nothing happens and we keep on going with the
            // loop body. We do so to ensure that printing is handled before keypresses.
            if self.check_and_print_app_output() {
                need_refresh = true;
                continue;
            }

            // Handles the cleanup of any application task that has finished running, returns whether we need
            // a new prompt or need to refresh the screen.
            let (need_refresh_on_task_event, need_prompt_on_task_event) = self.task_handler()?;

            // Print prompt or refresh the screen based on needs.
            if need_prompt || need_prompt_on_task_event {
                self.redisplay_prompt();
                need_prompt = false;
            }
            if need_refresh || need_refresh_on_task_event {
                self.terminal.lock().refresh_display(0);
                need_refresh = false;
            }

            // Looks at the input queue from the window manager
            // If it has unhandled items, it handles them with the match
            // If it is empty, it proceeds directly to the next loop iteration
            match self.terminal.lock().get_key_event() {
                Some(ev) => {
                    match ev {
                        // Returns from the main loop.
                        Event::ExitEvent => {
                            trace!("exited terminal");
                            return Ok(());
                        }

                        Event::ResizeEvent(ref _rev) => {
                            self.terminal.lock().refresh_display(self.left_shift); // application refreshes display after resize event is received
                        }

                        // Handles ordinary keypresses
                        Event::InputEvent(ref input_event) => {
                            self.key_event_producer.write_one(input_event.key_event);
                        }
                        _ => { }
                    };
                },
                _ => { }
            };

            let mut need_refresh = false;
            loop {
                let locked_consumer = self.key_event_consumer.lock();
                if let Some(ref key_event_consumer) = locked_consumer.deref() {
                    if let Some(key_event) = key_event_consumer.read_one() {
                        mem::drop(locked_consumer); // drop the lock so that we can invoke the method on the next line
                        if let Err(e) = self.handle_key_event(key_event) {
                            error!("{}", e);
                        }
                        if key_event.action == KeyAction::Pressed { need_refresh = true; }
                    } else { // currently the key event queue is empty, break the loop
                        break;
                    }
                } else { // currently the key event queue is taken by an application
                    break;
                }
            }
            if need_refresh {
                self.terminal.lock().refresh_display(self.left_shift);
            } else {
                scheduler::schedule(); // yield the CPU if nothing to do
            }
        }
    }
}

/// Shell internal command related methods.
impl Shell {
    /// Check if the current command line is a shell internal command.
    fn is_internal_command(&self) -> bool {
        let mut iter = self.cmdline.split_whitespace();
        if let Some(cmd) = iter.next() {
            match cmd.as_ref() {
                "jobs" => return true,
                "fg" => return true,
                "bg" => return true,
                "clear" => return true,
                _ => return false
            }
        }
        false
    }

    /// Execute the command line as an internal command. If the current command line fails to
    /// be a shell internal command, this function does nothing.
    fn execute_internal(&mut self) -> Result<(), &'static str> {
        let cmdline_copy = self.cmdline.clone();
        let mut iter = cmdline_copy.split_whitespace();
        if let Some(cmd) = iter.next() {
            match cmd.as_ref() {
                "jobs" => self.execute_internal_jobs(),
                "fg" => self.execute_internal_fg(),
                "bg" => self.execute_internal_bg(),
                "clear" => self.execute_internal_clear(),
                _ => Ok(())
            }
        } else {
            Ok(())
        }
    }

    fn execute_internal_clear(&mut self) -> Result<(), &'static str> {
        self.terminal.lock().clear();
        self.clear_cmdline(false)?;
        self.redisplay_prompt();
        Ok(())
    }

    /// Execute `bg` command. It takes a job number and runs the in the background.
    fn execute_internal_bg(&mut self) -> Result<(), &'static str> {
        let cmdline_copy = self.cmdline.clone();
        let mut iter = cmdline_copy.split_whitespace();
        iter.next();
        let args: Vec<&str> = iter.collect();
        if args.len() != 1 {
            self.terminal.lock().print_to_terminal("Usage: bg %job_num\n".to_string());
            return Ok(());
        }
        if let Some('%') = args[0].chars().nth(0) {
            let job_num = args[0].chars().skip(1).collect::<String>();
            if let Ok(job_num) = job_num.parse::<isize>() {
                if let Some(job) = self.jobs.get_mut(&job_num) {
                    for task_ref in &job.tasks {
                        if !task_ref.lock().has_exited() {
                            task_ref.unblock();
                        }
                        job.status = JobStatus::Running;
                    }
                    self.clear_cmdline(false)?;
                    self.redisplay_prompt();
                    return Ok(());
                }
                self.terminal.lock().print_to_terminal(format!("No job number {} found!\n", job_num).to_string());
                return Ok(());
            }
        }
        self.terminal.lock().print_to_terminal("Usage: bg %job_num\n".to_string());
        Ok(())
    }

    /// Execute `fg` command. It takes a job number and runs the job in the foreground.
    fn execute_internal_fg(&mut self) -> Result<(), &'static str> {
        let cmdline_copy = self.cmdline.clone();
        let mut iter = cmdline_copy.split_whitespace();
        iter.next();
        let args: Vec<&str> = iter.collect();
        if args.len() != 1 {
            self.terminal.lock().print_to_terminal("Usage: fg %job_num\n".to_string());
            return Ok(());
        }
        if let Some('%') = args[0].chars().nth(0) {
            let job_num = args[0].chars().skip(1).collect::<String>();
            if let Ok(job_num) = job_num.parse::<isize>() {
                if let Some(job) = self.jobs.get_mut(&job_num) {
                    self.fg_job_num = Some(job_num);
                    for task_ref in &job.tasks {
                        if !task_ref.lock().has_exited() {
                            task_ref.unblock();
                        }
                        job.status = JobStatus::Running;
                    }
                    return Ok(());
                }
                self.terminal.lock().print_to_terminal(format!("No job number {} found!\n", job_num).to_string());
                return Ok(());
            }
        }
        self.terminal.lock().print_to_terminal("Usage: fg %job_num\n".to_string());
        Ok(())
    }

    /// Execute `jobs` command. It lists all jobs.
    fn execute_internal_jobs(&mut self) -> Result<(), &'static str> {
        for (job_num, job_ref) in self.jobs.iter() {
            let status = match &job_ref.status {
                JobStatus::Running => "running",
                JobStatus::Stopped => "stopped"
            };
            self.terminal.lock().print_to_terminal(format!("[{}] [{}] {}\n", job_num, status, job_ref.cmd).to_string());
        }
        if self.jobs.is_empty() {
            self.terminal.lock().print_to_terminal("No running or stopped jobs.\n".to_string());
        }
        self.clear_cmdline(false)?;
        self.redisplay_prompt();
        Ok(())
    }
}


/// Start a new shell. Shell::start() is an infinite loop, so normally we do not return from this function.
fn shell_loop(mut _dummy: i32) -> Result<(), &'static str> {

    Shell::new()?.start()?;
    Ok(())
}
