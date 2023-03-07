//! This crate maintains rendering of the windows and the mouse. It defines a `WindowManager` structure and initializes instance of it.
//! 
//! The `WindowManager` holds a vector of `Window`, which to be rendered to the front buffer, and their rendering order, it also hold information about the mouse.
//! 'WindowManager' own's a `VirtualFramebuffer` which acts like a back buffer and also owns a `PhysicalFramebuffer` which acts like a front buffer.
//! The window manager will iterate through the windows copying their content onto `VirtualFramebuffer`, then it will render the mouse and then finally it will copy `VirtualFramebuffer` onto `PhysicalFramebuffer`, which will update the screen with a new frame.


#![no_std]
#![feature(slice_ptr_get)]
#![feature(slice_flatten)]
extern crate alloc;
extern crate device_manager;
extern crate hpet;
extern crate memory;
extern crate mouse;
extern crate mouse_data;
extern crate multicore_bringup;
extern crate scheduler;
extern crate spin;
extern crate task;
pub mod units;
pub mod framebuffer;
pub mod window;
use alloc::sync::Arc;
use spin::{Mutex, Once};

use event_types::Event;
use mpmc::Queue;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_BASIC};
use mouse_data::MouseEvent;
use units::*;
use framebuffer::*;
use window::*;

/// Default window manager
pub static WINDOW_MANAGER: Once<Mutex<WindowManager>> = Once::new();

static SCREEN_WIDTH: usize = 1024;
static SCREEN_HEIGHT: usize = 768;

pub type Color = u32;
pub static DEFAULT_BORDER_COLOR: Color = 0x141414;
pub static DEFAULT_TEXT_COLOR: Color = 0xFBF1C7;
pub static DEFAULT_WINDOW_COLOR: Color = 0x3C3836;

static MOUSE_POINTER_IMAGE: [[u32; 11]; 18] = {
    const T: u32 = 0xFF0000;
    const C: u32 = 0x000000; // Cursor
    const B: u32 = 0xFFFFFF; // Border
    [
        [B, T, T, T, T, T, T, T, T, T, T],
        [B, B, T, T, T, T, T, T, T, T, T],
        [B, C, B, T, T, T, T, T, T, T, T],
        [B, C, C, B, T, T, T, T, T, T, T],
        [B, C, C, C, B, T, T, T, T, T, T],
        [B, C, C, C, C, B, T, T, T, T, T],
        [B, C, C, C, C, C, B, T, T, T, T],
        [B, C, C, C, C, C, C, B, T, T, T],
        [B, C, C, C, C, C, C, C, B, T, T],
        [B, C, C, C, C, C, C, C, C, B, T],
        [B, C, C, C, C, C, C, B, B, B, B],
        [B, C, C, C, C, C, B, T, T, T, T],
        [B, C, C, B, C, C, B, T, T, T, T],
        [B, C, B, T, B, C, C, B, T, T, T],
        [B, B, T, T, B, C, C, B, T, T, T],
        [B, T, T, T, T, B, C, C, B, T, T],
        [T, T, T, T, T, B, C, B, B, T, T],
        [T, T, T, T, T, T, B, T, T, T, T],
    ]
};

#[derive(PartialEq, Eq)]
pub enum Holding {
    Background,
    Nothing,
    Window(usize),
}

impl Holding {
    fn nothing(&self) -> bool {
        *self == Holding::Nothing
    }

    fn backgrond(&self) -> bool {
        *self == Holding::Background
    }

    fn window(&self) -> bool {
        !self.nothing() && !self.backgrond()
    }
}
/// The window manager, maintains windows, and the mouse, renders final frame to the screen.
pub struct WindowManager {
    /// Windows that are on the screen
    windows: Vec<Arc<Mutex<Window>>>,
    /// Rendering order for the windows
    window_rendering_order: Vec<usize>,
    /// Staging framebuffer
    staging_framebuffer: StagingFramenbuffer,
    /// Frontbuffer
    p_framebuffer: PhysicalFramebuffer,
    /// Width, height and position of the mouse
    mouse: Rect,
    /// Previous position of the mouse
    prev_mouse_pos: ScreenPos,
    /// What's currently held by the mouse
    mouse_holding: Holding,
    /// Holds the index of the active window/last element in the `window_rendering_order`
    active_window_index: usize,
}

impl WindowManager {
    /// Initializes the window manager, returns keyboard and mouse producer for the I/O devices
    pub fn init() -> Result<(Queue<Event>, Queue<Event>), &'static str> {
        let p_framebuffer = PhysicalFramebuffer::init_front_buffer()?;
        let staging_framebuffer = StagingFramenbuffer::new(p_framebuffer.width(), p_framebuffer.height(),p_framebuffer.stride())?;
        // FIXME: Don't use magic numbers,
        let mouse = Rect::new(11, 18, 200, 200);

        let window_manager = WindowManager {
            windows: Vec::new(),
            window_rendering_order: Vec::new(),
            staging_framebuffer,
            p_framebuffer,
            mouse,
            prev_mouse_pos: mouse.to_screen_pos(),
            mouse_holding: Holding::Nothing,
            active_window_index: usize::MAX,
        };
        WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));
        let key_consumer: Queue<Event> = Queue::with_capacity(100);
        let key_producer = key_consumer.clone();

        let mouse_consumer: Queue<Event> = Queue::with_capacity(100);
        let mouse_producer = mouse_consumer.clone();
        spawn::new_task_builder(port_loop, (key_consumer, mouse_consumer))
            .name("port_loop".to_string())
            .pin_on_core(0)
            .spawn()?;
        Ok((key_producer, mouse_producer))
    }

    
    /// Iterates through the `window_rendering_order`, gets the particular `Window` from `self.windows`
    /// and then locks it to hold the lock until we are done rendering that particular window into
    /// backbuffer
    fn draw_windows(&mut self) -> Result<(),&'static str> {
        for order in self.window_rendering_order.iter() {
            if let Some(mut window) = self
                .windows
                .get(*order)
                .and_then(|window| Some(window.lock()))
            {
                // Extra safety measure: For applications that render fast as possible we sometimes can't `fill`
                // which means we couldn't resize their framebuffers at correct time this extra checks handles that.
                if window.resized() {
                    // Notes to @ouz:
                    // * Why are we doing this here, instead of when the window is resized?
                    // * In general, shouldn't the window manager *not* modify the content of the window?
                    //   That seems to violate some sort of privacy/correctness-related invariant.
                    //
                    window.fill(DEFAULT_WINDOW_COLOR)?;
                }
                let visible_window = window.rect().visible_rect();
                let relative_visible_window = window.relative_visible_rect();
                let screen_rows = FramebufferRowIter::new(
                    &mut self.staging_framebuffer,
                    visible_window,
                );
                // To handle rendering when the window is partially outside the screen we use relative version of visible rect
                let window_rows = FramebufferRowIter::new(
                    &mut window.frame_buffer,
                    relative_visible_window,
                );

                for (screen_row, window_row) in screen_rows.zip(window_rows) {
                    screen_row.copy_from_slice(window_row);
                }
            }
        }
        Ok(())
    }

    /// Draws the mouse, if mouse is partially outside the screen it will only draw parts that are inside the screen.
    fn draw_mouse(&mut self) {
        // Rectangle of visible parts of the mouse
        let visible_mouse = self.mouse.visible_rect();

        let screen_rows = FramebufferRowIter::new(
            &mut self.staging_framebuffer,
            visible_mouse,
        );

        for (screen_row, mouse_image_row) in screen_rows.zip(MOUSE_POINTER_IMAGE.iter()){
            for (screen_pixel, mouse_pixel) in screen_row.iter_mut().zip(mouse_image_row.iter()) {
                if mouse_pixel != &0xFF0000 {
                    *screen_pixel = *mouse_pixel;
                }
            }
        }
    }

    /// Returns current screen width and height
    pub fn screen_size(&self) -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub fn set_mouse_pos(&mut self, screen_positon: &ScreenPos) {
        self.mouse.x = screen_positon.x as isize;
        self.mouse.y = screen_positon.y as isize;
    }

    pub fn set_window_event(&mut self, event: Event) {
        if let Some(window) = self.windows.get_mut(self.active_window_index) {
            if window.lock().receive_events{
                window.lock().push_event(event);
            }
        }
    }

    /// Updates backbuffer.
    /// Clears the whole screen by calling `blank` then
    /// draws each window and then the mouse. 
    fn update(&mut self) -> Result<(),&'static str> {
        self.staging_framebuffer.blank();
        self.draw_windows()?;
        self.draw_mouse();
        Ok(())
    }

    fn calculate_next_mouse_pos(
        &self,
        current_position: ScreenPos,
        relative_offset: ScreenPos,
    ) -> ScreenPos {
        let mut new_pos = relative_offset + current_position;

        // handle left
        new_pos.x = core::cmp::max(new_pos.x, 0);
        // handle right
        new_pos.x = core::cmp::min(
            new_pos.x,
            self.staging_framebuffer.width() as i32 - MOUSE_VISIBLE_GAP,
        );

        // handle top
        new_pos.y = core::cmp::max(new_pos.y, 0);
        // handle bottom
        new_pos.y = core::cmp::min(
            new_pos.y,
            self.staging_framebuffer.height() as i32 - MOUSE_VISIBLE_GAP,
        );

        new_pos
    }

    /// Returns currently active window
    fn active_window(&mut self) -> Option<&mut Arc<Mutex<Window>>> {
        if let Some(window) = self.windows.get_mut(self.active_window_index) {
            Some(window)
        } else {
            None
        }
    }

    fn update_mouse_position(&mut self, raw_x: i32, raw_y: i32) {
        let relative_offset = ScreenPos::new(raw_x, raw_y);
        self.prev_mouse_pos = self.mouse.to_screen_pos();
        let new_pos = self.calculate_next_mouse_pos(self.mouse.to_screen_pos(), relative_offset);

        self.set_mouse_pos(&new_pos);
    }

    fn set_window_non_active(&mut self, window_index: usize) {
        if let Some(window) = self.windows.get_mut(window_index) {
            window.lock().active = false;
        }
    }

    // TODO: This can be greatly simplfied, instead of having one big function cut this into smaller ones.
    fn handle_mouse_events_on_windows(&mut self, screen_position: ScreenPos, mouse_event: &MouseEvent) {
        if !mouse_event.buttons.left() && !mouse_event.buttons.right() {
            self.mouse_holding = Holding::Nothing;
            if let Some(window) = self.active_window() {
                if window.lock().resizing {
                    window.lock().resizing = false;
                }
            }
        }
        if mouse_event.buttons.left() && !mouse_event.buttons.right() {
            match self.mouse_holding {
                // TODO: Add functionality of being able to grab no window/background.
                Holding::Background => {}
                Holding::Nothing => {
                    // We are cloning this value because we will use it to iterate through our windows while editing the original one
                    let rendering_order = self.window_rendering_order.clone();
                    // `iter_index` = index of the window in `self.window_rendering_order`
                    // `window_index` = index of the window in `self.windows`
                    for (iter_index, &window_index) in rendering_order.iter().enumerate().rev() {
                        let window = &mut self.windows[window_index].clone();
                        if window.lock().rect().detect_collision(&Rect::new(4, 4, self.mouse.x, self.mouse.y)) {
                            // If colliding window is not active one make it active
                            // we first remove colliding window from it's position in
                            // window_rendering_order, then push it to the back of
                            // window_rendering_order, this way we don't have to do any special sorting
                            if window_index != self.active_window_index {
                                self.set_window_non_active(self.active_window_index);
                                self.active_window_index = window_index;
                                self.window_rendering_order.remove(iter_index);
                                self.window_rendering_order.push(window_index);
                                window.lock().active = true;
                            }
                            // If user is holding the window from it's title border pos
                            // it means user wants to move the window
                            if window
                                .lock()
                                .dynamic_title_border_pos()
                                .detect_collision(&Rect::new(4, 4, self.mouse.x, self.mouse.y))
                            {
                                self.mouse_holding = Holding::Window(window_index);
                            }
                            break;
                        }
                        self.mouse_holding = Holding::Nothing;
                    }
                    // If couldn't hold onto anything we must have hold onto background
                    if self.mouse_holding.nothing() {
                        self.mouse_holding = Holding::Background
                    }
                }
                Holding::Window(i) => {
                    // These calculations are required because we do want finer control
                    // over a window's movement.
                    let prev_mouse_pos = self.prev_mouse_pos;
                    let next_mouse_pos =
                        self.calculate_next_mouse_pos(prev_mouse_pos, screen_position);
                    let window = &mut self.windows[i];
                    let window_rect = window.lock().rect();
                    let diff = next_mouse_pos - prev_mouse_pos;
                    let mut new_pos = diff + window_rect.to_screen_pos();

                    //handle left
                    if (new_pos.x + (window_rect.width as i32 - WINDOW_VISIBLE_GAP as i32)) < 0 {
                        new_pos.x = -(window_rect.width as i32 - WINDOW_VISIBLE_GAP);
                    }

                    //handle right
                    if (new_pos.x + WINDOW_VISIBLE_GAP) > self.staging_framebuffer.width() as i32 {
                        new_pos.x = SCREEN_WIDTH as i32 - WINDOW_VISIBLE_GAP
                    }

                    //handle top
                    new_pos.y = core::cmp::max(new_pos.y, 0);

                    // handle bottom
                    if new_pos.y + WINDOW_VISIBLE_GAP > self.staging_framebuffer.height() as i32 {
                        new_pos.y = (SCREEN_HEIGHT as i32 - WINDOW_VISIBLE_GAP) as i32;
                    }

                    window.lock().set_screen_pos(&new_pos);
                }
            }
        } else if mouse_event.buttons.right() {
            for &i in self.window_rendering_order.iter().rev() {
                let window = &mut self.windows[i].lock();
                if window.rect().detect_collision(&Rect::new(
                    self.mouse.width,
                    self.mouse.height,
                    self.mouse.x,
                    self.mouse.y,
                )) {
                    window.resizing = true;
                    window
                        .resize_window(screen_position.x, screen_position.y);
                    window.reset_drawable_area();
                    window.reset_title_pos_and_border();
                    break;
                }
            }
        }
    }

    /// Performs the final blit of the staging framebuffer to the physical framebuffer,
    /// making its changes visible on screen.
    ///
    /// If and when we support double-buffering, this is where the buffers will be flipped/swapped.
    fn render(&mut self) {
        self.p_framebuffer
            .buffer_mut()
            .copy_from_slice(&self.staging_framebuffer.buffer());
    }
}

fn port_loop(
    (key_consumer, mouse_consumer): (Queue<Event>, Queue<Event>),
) -> Result<(), &'static str> {
    let window_manager = WINDOW_MANAGER.get().ok_or("Unable to get WindowManager")?;

    loop {
        let event_opt = key_consumer
            .pop()
            .or_else(|| mouse_consumer.pop())
            .or_else(|| {
                scheduler::schedule();
                None
            });

        if let Some(event) = event_opt {
            match event {
                Event::MouseMovementEvent(ref mouse_event) => {
                    window_manager.lock().set_window_event(Event::MouseMovementEvent(mouse_event.clone()));
                    let movement = &mouse_event.movement;
                    let mut x = (movement.x_movement as i8) as isize;
                    let mut y = (movement.y_movement as i8) as isize;
                    while let Some(next_event) = mouse_consumer.pop() {
                        match next_event {
                            Event::MouseMovementEvent(ref next_mouse_event) => {
                                if next_mouse_event.movement.scroll_movement
                                    == mouse_event.movement.scroll_movement
                                    && next_mouse_event.buttons.left() == mouse_event.buttons.left()
                                    && next_mouse_event.buttons.right()
                                        == mouse_event.buttons.right()
                                    && next_mouse_event.buttons.fourth()
                                        == mouse_event.buttons.fourth()
                                    && next_mouse_event.buttons.fifth()
                                        == mouse_event.buttons.fifth()
                                {
                                    x += (next_mouse_event.movement.x_movement as i8) as isize;
                                    y += (next_mouse_event.movement.y_movement as i8) as isize;
                                }
                            }

                            _ => {
                                break;
                            }
                        }
                    }
                    if x != 0 || y != 0 {
                        window_manager
                            .lock()
                            .update_mouse_position(x as i32, -(y as i32));
                    }
                    window_manager
                        .lock()
                        .handle_mouse_events_on_windows(ScreenPos::new(x as i32, -(y as i32)), &mouse_event);
                }
                Event::KeyboardEvent(ref _input_event) => {
                    window_manager.lock().set_window_event(event);
                }
                _ => (),
            }
        }

        window_manager.lock().update()?;
        window_manager.lock().render();
    }
    Ok(())
}
