#![no_std]
extern crate alloc;
extern crate font;
extern crate porthole;
extern crate spin;

use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use porthole::units::Rect;
use porthole::{
    window, DEFAULT_BORDER_COLOR, DEFAULT_TEXT_COLOR, DEFAULT_WINDOW_COLOR,
};
use spin::{Mutex, RwLockReadGuard};

extern crate device_manager;
extern crate hpet;
extern crate memory;
extern crate mouse;
extern crate mouse_data;
extern crate multicore_bringup;
extern crate scheduler;
extern crate task;

use alloc::string::{String, ToString};
use hpet::{get_hpet, Hpet};
use memory::{BorrowedMappedPages, Mutable};
use window::*;
// Useful toy application that shows the real time performance
pub struct FpsCounter {
    window: Arc<Mutex<Window>>,
    hpet: RwLockReadGuard<'static, BorrowedMappedPages<Hpet, Mutable>>,
    time_it_took_to_render: u64,
    timer_freq: u64,
    total_frames: u64,
    total_time: u64,
    avg_fps: u64,
    avg_time: u64,
    avg_fps_str: String,
    avg_time_str: String,
}

impl FpsCounter {
    pub fn new() -> Result<Self, &'static str> {
        let window = Window::new_window(
            &Rect::new(360, 360, 0, 0),
            Some(format!("FpsCounter")),
            false,
        )?;
        let hpet = get_hpet().ok_or("Unable to get hpet")?;
        let time_it_took_to_render = hpet.get_counter();
        let timer_freq = hpet.counter_period_femtoseconds() as u64;
        window.lock().fill(DEFAULT_WINDOW_COLOR)?;
        Ok(Self {
            window,
            hpet,
            time_it_took_to_render,
            timer_freq,
            total_frames: 0,
            total_time: 0,
            avg_fps: 0,
            avg_time: 0,
            avg_fps_str: format!("Frames per second:"),
            avg_time_str: format!("Median frame time in micro seconds:"),
        })
    }

    fn calculate_next_frame_time(&mut self) {
        let time = self.hpet.get_counter();
        let diff = (time - self.time_it_took_to_render) * self.timer_freq / 1_000_000_000;
        self.total_time += diff;
        self.time_it_took_to_render = time;
        self.total_frames += 1;
    }

    fn reset_counters(&mut self) {
        // this equals to a second
        if self.total_time >= 1_000_000 {
            self.avg_fps = self.total_frames;
            self.avg_time = self.total_time / self.total_frames;
            self.total_time = 0;
            self.total_frames = 0;
        }
    }

    pub fn run(&mut self) -> Result<(), &'static str> {
        let mut counter = 0;
        self.window.lock().fill(DEFAULT_WINDOW_COLOR)?;
        loop {
            self.calculate_next_frame_time();
            self.reset_counters();
            if counter == 1 {
                counter = 0;
                self.draw()?;
                continue;
            } else {
                scheduler::schedule();
                counter += 1;
            }
        }
        Ok(())
    }

    fn draw(&mut self) -> Result<(), &'static str> {
        if self.window.lock().resized() {
            self.window.lock().fill(DEFAULT_WINDOW_COLOR)?;
        }
        self.window
            .lock()
            .display_window_title(DEFAULT_TEXT_COLOR, DEFAULT_BORDER_COLOR)?;
        self.print_avg_fps()?;
        self.print_avg_time()?;
        Ok(())
    }

    fn print_avg_fps(&mut self) -> Result<(), &'static str> {
        let mut drawable_area = self.window.lock().drawable_area().to_relative_pos();
        let avg_fps = self.avg_fps.to_string();

        self.window.lock().print_string_line(
            &drawable_area,
            &self.avg_fps_str,
            0xF8FF0E,
            DEFAULT_WINDOW_COLOR,
        )?;

        drawable_area.x = (CHARACTER_WIDTH * self.avg_fps_str.len()) as u32;
        self.window.lock().print_string_line(
            &drawable_area,
            &avg_fps,
            0x20F065,
            DEFAULT_WINDOW_COLOR,
        )?;
        Ok(())
    }

    fn print_avg_time(&mut self) -> Result<(), &'static str> {
        let mut drawable_area = self.window.lock().drawable_area().to_relative_pos();
        drawable_area.y += (CHARACTER_HEIGHT + 1) as u32;
        let avg_time = self.avg_time.to_string();
        // Prints default text for `avg_time_str`
        self.window.lock().print_string_line(
            &drawable_area,
            &self.avg_time_str,
            0xF8FF0E,
            DEFAULT_WINDOW_COLOR,
        )?;

        drawable_area.x = (CHARACTER_WIDTH * self.avg_time_str.len()) as u32;
        // Prints current avg time
        self.window.lock().print_string_line(
            &drawable_area,
            &avg_time,
            0x20F065,
            DEFAULT_WINDOW_COLOR,
        )?;

        Ok(())
    }
}

pub fn main(_args: Vec<isize>) -> isize {
    {
        let _task_ref = match spawn::new_task_builder(fps_counter_loop, ())
            .name("fps_counter_loop".to_string())
            .spawn()
        {
            Ok(task_ref) => task_ref,
            Err(err) => {
                log::error!("{}", err);
                log::error!("failed to spawn fps_counter");
                return -1;
            }
        };
    }

    // block this task, because it never needs to actually run again
    task::with_current_task(|t| t.block())
        .expect("fps_counter::main(): failed to get current task")
        .expect("fps_counter:main(): failed to block the maintask");
    scheduler::schedule();

    loop {
        //log:: warn!("BUG: blocked shell task was scheduled in unexpectedly");
    }
}
pub fn fps_counter_loop(mut _dummy: ()) -> Result<(), &'static str> {
    FpsCounter::new()?.run()?;
    Ok(())
}
