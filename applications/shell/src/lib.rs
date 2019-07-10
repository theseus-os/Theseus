//! Shell with event-driven architecture
//! Commands that can be run are the names of the crates in the applications directory
//! 
//! The shell has the following responsibilities: handles key events delivered from terminal, manages terminal display,
//! spawns and manages tasks, and records previously executed user commands.
//! 
//! Problem: Currently there's no upper bound to the user command line history.
//!
//! Acknowledgement: Most of the functions are adopted from the `Terminal` implemented by Andrew Pham <apham727@gmail.com>.

#![no_std]
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
extern crate fs_node;
extern crate path;
extern crate root;

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
use fs_node::FileOrDir;
use libterm::Terminal;

pub const APPLICATIONS_NAMESPACE_PATH: &'static str = "/namespaces/default/applications";

/// A main function that spawns a new shell and waits for the shell loop to exit before returning an exit value
#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    let terminal =  match Terminal::new() {
        Ok(_terminal) => { _terminal }
        Err(err) => {
            error!("{}", err);
            error!("could not create terminal instance");
            return -1;
        }
    };

    let _task_ref = match KernelTaskBuilder::new(shell_loop, terminal)
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
    current_task_ref: Option<TaskRef>,
    /// Vector that stores the history of commands that the user has entered
    command_history: Vec<String>,
    /// Variable used to track the net number of times the user has pressed up/down to cycle through the commands
    /// ex. if the user has pressed up twice and down once, then command shift = # ups - # downs = 1 (cannot be negative)
    history_index: usize,
    /// When someone enters some commands, but before pressing `enter` it presses `up` to see previous commands,
    /// we must push it to command_history. We don't want to push it twice.
    buffered_cmd_recorded: bool,
    /// the terminal that is bind with the shell instance
    terminal: Terminal
}

impl Shell {
    /// Create a new shell. Must provide a terminal to bind with the new shell in the argument.
    fn new(terminal: Terminal) -> Shell {
        Shell {
            current_task_ref: None,
            command_history: Vec::new(),
            history_index: 0,
            buffered_cmd_recorded: false,
            terminal
        }
    }


    fn goto_previous_command(&mut self) {
        if self.history_index == self.command_history.len() {
            return;
        }
        self.terminal.refresh_prompt_if_needed();
        if self.history_index == 0 {
            let previous_input = self.terminal.get_cmdline();
            if !previous_input.is_empty() {
                self.command_history.push(previous_input);
                self.history_index += 1;
                self.buffered_cmd_recorded = true;
            }
        }
        self.history_index += 1;
        let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
        self.terminal.set_cmdline(selected_command);
    }

    fn goto_next_command(&mut self) {
        if self.history_index == 0 {
            return;
        }
        if self.history_index == 1 {
            if self.buffered_cmd_recorded {
                // command_histroy has at least one element. safe to unwrap here.
                let selected_command = self.command_history.pop().unwrap();
                self.terminal.set_cmdline(selected_command);
                self.history_index -= 1;
                self.buffered_cmd_recorded = false;
                return;
            }
        }
        self.history_index -=1;
        if self.history_index == 0 {
            self.terminal.clear_cmdline();
            return;
        }
        let selected_command = self.command_history[self.command_history.len() - self.history_index].clone();
        self.terminal.set_cmdline(selected_command);
    }

    fn handle_key_event(&mut self, keyevent: KeyEvent) -> Result<(), &'static str> {       
        // EVERYTHING BELOW HERE WILL ONLY OCCUR ON A KEY PRESS (not key release)
        if keyevent.action != KeyAction::Pressed {
            return Ok(()); 
        }

        // Ctrl+C signals the main loop to exit the task
        if keyevent.modifiers.control && keyevent.keycode == Keycode::C {
            let task_ref_copy = match self.current_task_ref {
                Some(ref task_ref) => task_ref.clone(), 
                None => {
                    self.terminal.clear_cmdline();
                    self.terminal.clear_buffer_string();
                    self.terminal.print_to_terminal("^C\n".to_string());
                    self.terminal.redisplay_prompt();
                    return Ok(());
                }
            };
            match task_ref_copy.kill(KillReason::Requested) {
                Ok(_) => {
                    if let Err(e) = runqueue::remove_task_from_all(&task_ref_copy) {
                        error!("Killed task but could not remove it from runqueue: {}", e);
                    }
                }
                Err(e) => error!("Could not kill task, error: {}", e),
            }
            return Ok(());
        }

        // Tracks what the user does whenever she presses the backspace button
        if keyevent.keycode == Keycode::Backspace  {
            self.terminal.erase_left_cmdline();
            return Ok(());
        }

        if keyevent.keycode == Keycode::Delete {
            self.terminal.erase_right_cmdline();
            return Ok(());
        }

        // Attempts to run the command whenever the user presses enter and updates the cursor tracking variables 
        if keyevent.keycode == Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            let cmdline = self.terminal.get_cmdline();
            if cmdline.len() == 0 {
                // reprints the prompt on the next line if the user presses enter and hasn't typed anything into the prompt
                self.terminal.print_to_terminal("\n".to_string());
                self.terminal.redisplay_prompt();
                return Ok(());
            } else if self.current_task_ref.is_some() { // prevents the user from trying to execute a new command while one is currently running
                self.terminal.print_to_terminal("Wait until the current command is finished executing\n".to_string());
            } else {
                self.terminal.print_to_terminal("\n".to_string());
                self.command_history.push(cmdline.clone());
                self.command_history.dedup(); // Removes any duplicates
                self.history_index = 0;
                match self.eval_cmdline() {
                    Ok(new_task_ref) => { 
                        let task_id = {new_task_ref.lock().id};
                        self.current_task_ref = Some(new_task_ref);
                        terminal_print::add_child(task_id, self.terminal.get_producer_to_screen())?; // adds the terminal's print producer to the terminal print crate
                    }
                    Err(err) => {
                        let err_msg = match err {
                            AppErr::NotFound(command) => format!("{:?} command not found.\n", command),
                            AppErr::NamespaceErr      => format!("Failed to find directory of application executables.\n"),
                            AppErr::SpawnErr(e)       => format!("Failed to spawn new task to run command. Error: {}.\n", e),
                        };
                        self.terminal.print_to_terminal(err_msg);
                        self.terminal.redisplay_prompt();
                        self.terminal.clear_cmdline_without_erase();
                        return Ok(());
                    }
                }
            }
            // Clears the buffer for next command once current command starts executing
            self.terminal.clear_cmdline_without_erase();
            return Ok(());
        }

        // home, end, page up, page down, up arrow, down arrow for the input_event_manager
        if keyevent.keycode == Keycode::Home && keyevent.modifiers.control {
            self.terminal.move_screen_to_begin();
            return Ok(());
        }
        if keyevent.keycode == Keycode::End && keyevent.modifiers.control{
            self.terminal.move_screen_to_end();
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Up  {
            self.terminal.move_screen_line_up();
            return Ok(());
        }
        if keyevent.modifiers.control && keyevent.modifiers.shift && keyevent.keycode == Keycode::Down  {
            self.terminal.move_screen_line_down();
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageUp && keyevent.modifiers.shift {
            self.terminal.move_screen_page_up();
            return Ok(());
        }

        if keyevent.keycode == Keycode::PageDown && keyevent.modifiers.shift {
            self.terminal.move_screen_page_down();
            return Ok(());
        }

        // Cycles to the next previous command
        if  keyevent.keycode == Keycode::Up {
            self.goto_previous_command();
            return Ok(());
        }
        // Cycles to the next most recent command
        if keyevent.keycode == Keycode::Down {
            self.goto_next_command();
            return Ok(());
        }

        // Jumps to the beginning of the input string
        if keyevent.keycode == Keycode::Home {
            self.terminal.move_cursor_leftmost();
            return Ok(());
        }

        // Jumps to the end of the input string
        if keyevent.keycode == Keycode::End {
            self.terminal.move_cursor_rightmost();
            return Ok(());
        }

        // Adjusts the cursor tracking variables when the user presses the left and right arrow keys
        if keyevent.keycode == Keycode::Left {
            self.terminal.move_cursor_left();
            return Ok(());
        }

        if keyevent.keycode == Keycode::Right {
            self.terminal.move_cursor_right();
            return Ok(());
        }

        // Tracks what the user has typed so far, excluding any keypresses by the backspace and Enter key, which are special and are handled directly below
        if keyevent.keycode != Keycode::Enter && keyevent.keycode.to_ascii(keyevent.modifiers).is_some() {
            match keyevent.keycode.to_ascii(keyevent.modifiers) {
                Some(c) => {
                    // If currently we have a task running, insert it to the terminal buffer, otherwise
                    // to the cmdline.
                    if self.current_task_ref.is_some() {
                        self.terminal.insert_character_to_buffer(c);
                        return Ok(());
                    }
                    else {
                        self.terminal.insert_character_to_cmdline(c)?;
                    }
                },
                None => {
                    return Err("Couldn't get key event");
                }
            }
        }
        Ok(())
    }

    fn eval_cmdline(&mut self) -> Result<TaskRef, AppErr> {
        // Parse the cmdline
        let cmdline = self.terminal.get_cmdline();
        let mut args: Vec<String> = cmdline.split_whitespace().map(|s| s.to_string()).collect();
        let command = args.remove(0);

        // Check that the application actually exists
        let app_path = Path::new(APPLICATIONS_NAMESPACE_PATH.to_string());
        let app_list = match app_path.get(root::get_root()) {
            Some(FileOrDir::Dir(app_dir)) => {app_dir.lock().list()},
            _ => return Err(AppErr::NamespaceErr)
        };
        let mut executable = command.clone();
        executable.push_str(".o");
        if !app_list.contains(&executable) {
            return Err(AppErr::NotFound(command));
        }

        let taskref = match ApplicationTaskBuilder::new(Path::new(command))
            .argument(args)
            .spawn() {
                Ok(taskref) => taskref, 
                Err(e) => return Err(AppErr::SpawnErr(e.to_string()))
            };
        
        taskref.set_env(self.terminal.get_environment()); // Set environment variable of application to the same as terminal task

        // Gets the task id so we can reference this task if we need to kill it with Ctrl+C
        return Ok(taskref);
    }

    fn task_handler(&mut self) -> Result<(), &'static str> {
        let task_ref_copy = match self.current_task_ref.clone() {
            Some(task_ref) => task_ref,
            None => { return Ok(());}
        };
        let exit_result = task_ref_copy.take_exit_value();
        // match statement will see if the task has finished with an exit value yet
        match exit_result {
            Some(exit_val) => {
                match exit_val {
                    ExitValue::Completed(exit_status) => {
                        // here: the task ran to completion successfully, so it has an exit value.
                        // we know the return type of this task is `isize`,
                        // so we need to downcast it from Any to isize.
                        let val: Option<&isize> = exit_status.downcast_ref::<isize>();
                        info!("terminal: task returned exit value: {:?}", val);
                        if let Some(val) = val {
                            if *val < 0 {
                                self.terminal.print_to_terminal(format!("task returned error value {:?}\n", val));
                            }
                        }
                    },

                    ExitValue::Killed(KillReason::Requested) => {
                        self.terminal.print_to_terminal("^C\n".to_string());
                    },
                    // If the user manually aborts the task
                    ExitValue::Killed(kill_reason) => {
                        warn!("task was killed because {:?}", kill_reason);
                        self.terminal.print_to_terminal(format!("task was killed because {:?}\n", kill_reason));
                    }
                }
                
                terminal_print::remove_child(task_ref_copy.lock().id)?;
                // Resets the current task id to be ready for the next command
                self.current_task_ref = None;
                self.terminal.redisplay_prompt();
                // Pushes the keypresses onto the input_event_manager that were tracked whenever another command was running
                if !self.terminal.is_buffer_string_empty() {
                    self.terminal.consume_buffer_string()?;
                }
                // Resets the bool to true once the print prompt has been redisplayed
                self.terminal.refresh_display();
            },
        // None value indicates task has not yet finished so does nothing
        None => { /* WARNING: really should do nothing? */ },
        }
        return Ok(());
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
        loop {
            self.terminal.blink_cursor();

            // If there is anything from running applications to be printed, it printed on the screen and then
            // return true, so that the loop continues, otherwise nothing happens and we keep on going with the
            // loop body. We do so to ensure that printing is handled before keypresses.
            if self.terminal.check_and_print_app_output() { continue; }


            // Handles the cleanup of any application task that has finished running, including refreshing the display
            self.task_handler()?;
            self.terminal.refresh_prompt_if_needed();

            // Looks at the input queue from the window manager
            // If it has unhandled items, it handles them with the match
            // If it is empty, it proceeds directly to the next loop iteration
            let event = match self.terminal.get_key_event() {
                Some(ev) => {
                    ev
                },
                _ => { continue; }
            };

            match event {
                // Returns from the main loop so that the terminal object is dropped
                Event::ExitEvent => {
                    trace!("exited terminal");
                    self.terminal.close_window()?;
                    return Ok(());
                }

                Event::ResizeEvent(ref _rev) => {
                    self.terminal.refresh_display(); // application refreshes display after resize event is received
                }

                // Handles ordinary keypresses
                Event::InputEvent(ref input_event) => {
                    self.handle_key_event(input_event.key_event)?;
                    if input_event.key_event.action == KeyAction::Pressed {
                        // only refreshes the display on keypresses to improve display performance 
                        self.terminal.refresh_display();
                    }
                }
                _ => { }
            }
        }
    }
}


/// Start a new shell. Shell::start() is an infinite loop, so normally we do not return from this function.
fn shell_loop(mut terminal: Terminal) -> Result<(), &'static str> {

    terminal.initialize_screen()?;
    Shell::new(terminal).start()?;
    Ok(())
}
