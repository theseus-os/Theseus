#![no_std]

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
use alloc::sync::{Arc, Weak};
use log::{debug, info};
use spin::{Mutex, MutexGuard, Once};

use event_types::{Event, MousePositionEvent};
use keycodes_ascii::{KeyAction, KeyEvent, Keycode};
use mpmc::Queue;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH};
use hpet::get_hpet;
use memory::{BorrowedSliceMappedPages, Mutable, PhysicalAddress, PteFlags, PteFlagsArch};
use mouse_data::MouseEvent;
use task::{ExitValue, JoinableTaskRef, KillReason};
pub static WINDOW_MANAGER: Once<Mutex<WindowManager>> = Once::new();

static MOUSE_POINTER_IMAGE: [[u32; 18]; 11] = {
    const T: u32 = 0xFF0000;
    const C: u32 = 0x000000; // Cursor
    const B: u32 = 0xFFFFFF; // Border
    [
        [B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, B, T, T],
        [T, B, C, C, C, C, C, C, C, C, C, C, C, C, B, T, T, T],
        [T, T, B, C, C, C, C, C, C, C, C, C, C, B, T, T, T, T],
        [T, T, T, B, C, C, C, C, C, C, C, C, B, T, T, T, T, T],
        [T, T, T, T, B, C, C, C, C, C, C, C, C, B, B, T, T, T],
        [T, T, T, T, T, B, C, C, C, C, C, C, C, C, C, B, B, T],
        [T, T, T, T, T, T, B, C, C, C, C, B, B, C, C, C, C, B],
        [T, T, T, T, T, T, T, B, C, C, B, T, T, B, B, C, B, T],
        [T, T, T, T, T, T, T, T, B, C, B, T, T, T, T, B, B, T],
        [T, T, T, T, T, T, T, T, T, B, B, T, T, T, T, T, T, T],
        [T, T, T, T, T, T, T, T, T, T, B, T, T, T, T, T, T, T],
    ]
};

pub struct App {
    window: Arc<Mutex<Window>>,
    text: TextDisplay,
}

impl App {
    pub fn new(window: Arc<Mutex<Window>>, text: TextDisplay) -> Self {
        Self { window, text }
    }
    pub fn draw(&mut self) {
        self.window.lock().draw_rectangle(0x111FFF);
        self.text.print_string("slie", &mut self.window.lock());
    }
}

pub struct TextDisplay {
    width: usize,
    height: usize,
    next_col: usize,
    next_line: usize,
    text: String,
    fg_color: u32,
    bg_color: u32,
}

impl TextDisplay {
    pub fn new(
        width: usize,
        height: usize,
        next_col: usize,
        next_line: usize,
        text: String,
        fg_color: u32,
        bg_color: u32,
    ) -> Self {
        Self {
            width,
            height,
            next_col,
            next_line,
            text,
            fg_color,
            bg_color,
        }
    }

    pub fn print_string(&mut self, slice: &str, window: &mut MutexGuard<Window>) {
        let rect = window.rect;
        let buffer_width = rect.width / CHARACTER_WIDTH;
        let buffer_height = rect.height / CHARACTER_HEIGHT;
        let (x, y) = (rect.x, rect.y);

        let some_slice = slice.as_bytes();

        self.print_ascii_character(some_slice, window);
    }

    // TODO: Try to simplify this
    pub fn print_ascii_character(&mut self, slice: &[u8], window: &mut MutexGuard<Window>) {
        let rect = window.rect;
        let relative_x = rect.x;
        let relative_y = rect.y;
        let start_x = relative_x + (self.next_col as isize * CHARACTER_WIDTH as isize);
        let start_y = relative_y + (self.next_line as isize * CHARACTER_HEIGHT as isize);

        let buffer_width = rect.width;
        let buffer_height = rect.height;

        let off_set_x = 0;
        let off_set_y = 0;

        let mut j = off_set_x;
        let mut i = off_set_y;
        let mut z = 0;
        let mut index_j = j;
        loop {
            let x = start_x + j as isize;
            let y = start_y + i as isize;
            if j % CHARACTER_WIDTH == 0 {
                index_j = 0;
            }
            let color = if index_j >= 1 {
                let index = index_j - 1;
                let char_font = font::FONT_BASIC[slice[z] as usize][i];
                index_j += 1;
                if self.get_bit(char_font, index) != 0 {
                    self.fg_color
                } else {
                    self.bg_color
                }
            } else {
                index_j += 1;
                self.bg_color
            };
            window.draw_relative(x, y, color);

            j += 1;
            if j == CHARACTER_WIDTH
                || j % CHARACTER_WIDTH == 0
                || start_x + j as isize == buffer_width as isize
            {
                if slice.len() >= 1 && z < slice.len() - 1 {
                    z += 1;
                }

                if j >= CHARACTER_WIDTH * slice.len() && j % (CHARACTER_WIDTH * slice.len()) == 0 {
                    i += 1;
                    z = 0;
                    j = off_set_x;
                }

                if i == CHARACTER_HEIGHT || start_y + i as isize == buffer_height as isize {
                    break;
                }
            }
        }
    }
    fn get_bit(&self, char_font: u8, i: usize) -> u8 {
        char_font & (0x80 >> i)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub width: usize,
    pub height: usize,
    pub x: isize,
    pub y: isize,
}

impl Rect {
    fn new(width: usize, height: usize, x: isize, y: isize) -> Rect {
        Rect {
            width,
            height,
            x,
            y,
        }
    }

    fn start_x(&self) -> isize {
        self.x
    }

    fn end_x(&self) -> isize {
        self.x + self.width as isize
    }

    fn start_y(&self) -> isize {
        self.y
    }

    fn end_y(&self) -> isize {
        self.y + self.height as isize
    }

    fn detect_collision(&self, other: &Rect) -> bool {
        if self.x < other.end_x()
            && self.end_x() > other.x
            && self.y < other.end_y()
            && self.end_y() > other.y
        {
            true
        } else {
            false
        }
    }
}

pub struct FrameBuffer {
    width: usize,
    height: usize,
    buffer: BorrowedSliceMappedPages<u32, Mutable>,
}
impl FrameBuffer {
    fn init_front_buffer() -> Result<FrameBuffer, &'static str> {
        let graphic_info = multicore_bringup::GRAPHIC_INFO.lock();
        if graphic_info.physical_address() == 0 {
            return Err("wrong physical address for porthole");
        }
        let vesa_display_phys_start =
            PhysicalAddress::new(graphic_info.physical_address() as usize).ok_or("Invalid address");
        let buffer_width = graphic_info.width() as usize;
        let buffer_height = graphic_info.height() as usize;

        let framebuffer = FrameBuffer::new(
            buffer_width,
            buffer_height,
            Some(vesa_display_phys_start.unwrap()),
        )?;
        Ok(framebuffer)
    }

    pub fn new(
        width: usize,
        height: usize,
        physical_address: Option<PhysicalAddress>,
    ) -> Result<FrameBuffer, &'static str> {
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let size = width * height * core::mem::size_of::<u32>();
        let pages = memory::allocate_pages_by_bytes(size)
            .ok_or("could not allocate pages for a new framebuffer")?;

        let mapped_framebuffer = if let Some(address) = physical_address {
            // For best performance, we map the real physical framebuffer memory
            // as write-combining using the PAT (on x86 only).
            // If PAT isn't available, fall back to disabling caching altogether.
            let mut flags: PteFlagsArch = PteFlags::new().valid(true).writable(true).into();

            #[cfg(target_arch = "x86_64")]
            {
                if page_attribute_table::is_supported() {
                    flags = flags.pat_index(
                        page_attribute_table::MemoryCachingType::WriteCombining.pat_slot_index(),
                    );
                    info!("Using PAT write-combining mapping for real physical framebuffer memory");
                } else {
                    flags = flags.device_memory(true);
                    info!("Falling back to cache-disable mapping for real physical framebuffer memory");
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                flags = flags.device_memory(true);
            }

            let frames = memory::allocate_frames_by_bytes_at(address, size)
                .map_err(|_e| "Couldn't allocate frames for the final framebuffer")?;
            let fb_mp = kernel_mmi_ref
                .lock()
                .page_table
                .map_allocated_pages_to(pages, frames, flags)?;
            debug!("Mapped real physical framebuffer: {fb_mp:?}");
            fb_mp
        } else {
            kernel_mmi_ref
                .lock()
                .page_table
                .map_allocated_pages(pages, PteFlags::new().valid(true).writable(true))?
        };

        Ok(FrameBuffer {
            width,
            height,
            buffer: mapped_framebuffer
                .into_borrowed_slice_mut(0, width * height)
                .map_err(|(_mp, s)| s)?,
        })
    }

    pub fn draw_something(&mut self, x: isize, y: isize, col: u32) {
        if x > 0 && x < self.width as isize && y > 0 && y < self.height as isize {
            self.buffer[(self.width * y as usize) + x as usize] = col;
        }
    }

    pub fn get_pixel(&self, x: isize, y: isize) -> u32 {
        self.buffer[(self.width * y as usize) + x as usize]
    }

    pub fn draw_rectangle(&mut self, rect: &Rect) {
        for y in rect.start_y()..rect.end_y() {
            for x in rect.start_x()..rect.end_x() {
                if x > 0 && x < self.width as isize && y > 0 && y < self.height as isize {
                    self.draw_something(x, y, 0xF123999);
                }
            }
        }
    }

    pub fn blank(&mut self) {
        for pixel in self.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }

    pub fn blank_rect(&mut self, rect: &Rect) {
        for y in rect.y..rect.end_y() {
            for x in rect.x..rect.end_x() {
                self.draw_something(x, y, 0x000000);
            }
        }
    }

    fn copy_window_only(&mut self, window: &MutexGuard<Window>) {
        for y in 0..window.rect.height {
            for x in 0..window.rect.width {
                let pixel = window.frame_buffer.get_pixel(x as isize, y as isize);
                let x = x as isize;
                let y = y as isize;
                if (x + window.rect.x) > 0
                    && (window.rect.x + x) < self.width as isize
                    && (y + window.rect.y) > 0
                    && (y + window.rect.y) < self.height as isize
                {
                    self.draw_something(
                        x as isize + window.rect.x,
                        y as isize + window.rect.y,
                        pixel,
                    );
                }
            }
        }
    }
}

pub fn main(_args: Vec<String>) -> isize {
    let mouse_consumer = Queue::with_capacity(100);
    let mouse_producer = mouse_consumer.clone();
    let key_consumer = Queue::with_capacity(100);
    let key_producer = mouse_consumer.clone();
    WindowManager::init();
    device_manager::init(key_producer, mouse_producer).unwrap();

    let _task_ref = match spawn::new_task_builder(port_loop, (mouse_consumer, key_consumer))
        .name("port_loop".to_string())
        .spawn()
    {
        Ok(task_ref) => task_ref,
        Err(err) => {
            log::error!("{}", err);
            log::error!("failed to spawn shell");
            return -1;
        }
    };

    task::get_my_current_task().unwrap().block().unwrap();
    scheduler::schedule();

    loop {
        log::warn!("BUG: blocked shell task was scheduled in unexpectedly");
    }
}

pub struct WindowManager {
    windows: Vec<Weak<Mutex<Window>>>,
    v_framebuffer: FrameBuffer,
    p_framebuffer: FrameBuffer,
    pub mouse: Rect,
}

impl WindowManager {
    fn init() {
        let p_framebuffer = FrameBuffer::init_front_buffer().unwrap();
        let v_framebuffer =
            FrameBuffer::new(p_framebuffer.width, p_framebuffer.height, None).unwrap();
        let mouse = Rect::new(11, 18, 200, 200);

        let window_manager = WindowManager {
            windows: Vec::new(),
            v_framebuffer,
            p_framebuffer,
            mouse,
        };
        WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));
    }

    fn new_window(dimensions: &Rect) -> Arc<Mutex<Window>> {
        let mut manager = WINDOW_MANAGER.get().unwrap().lock();

        let buffer_width = manager.p_framebuffer.width as usize;
        let buffer_height = manager.p_framebuffer.height as usize;

        let window = Window::new(
            *dimensions,
            FrameBuffer::new(dimensions.width, dimensions.height, None).unwrap(),
        );
        let arc_window = Arc::new(Mutex::new(window));
        manager.windows.push(Arc::downgrade(&arc_window.clone()));
        arc_window
    }

    fn draw_windows(&mut self) {
        for window in self.windows.iter() {
            self.v_framebuffer
                .copy_window_only(&window.upgrade().unwrap().lock());
        }
        for window in self.windows.iter() {
            window.upgrade().unwrap().lock().blank();
        }
    }

    fn draw_mouse(&mut self) {
        let mouse = self.mouse;
        for y in mouse.y..mouse.y + mouse.height as isize {
            for x in mouse.x..mouse.x + mouse.width as isize {
                let color = MOUSE_POINTER_IMAGE[(x - mouse.x) as usize][(y - mouse.y) as usize];
                if color != 0xFF0000 {
                    self.v_framebuffer.draw_something(x, y, color);
                }
            }
        }
    }

    fn update(&mut self) {
        self.v_framebuffer.blank();
        self.draw_windows();
        self.draw_mouse();
    }

    fn update_mouse_position(&mut self, x: isize, y: isize) {
        let mut new_pos_x = self.mouse.x + x;
        let mut new_pos_y = self.mouse.y - y;

        // handle left
        if (new_pos_x + (self.mouse.width as isize / 2)) < 0 {
            new_pos_x = self.mouse.x;
        }
        // handle right
        if new_pos_x + (self.mouse.width as isize / 2) > self.v_framebuffer.width as isize {
            new_pos_x = self.mouse.x;
        }

        // handle top
        if new_pos_y < 0 {
            new_pos_y = self.mouse.y;
        }

        // handle bottom
        if new_pos_y + (self.mouse.height as isize / 2) > self.v_framebuffer.height as isize {
            new_pos_y = self.mouse.y;
        }

        self.mouse.x = new_pos_x;
        self.mouse.y = new_pos_y;
    }

    fn drag_windows(&mut self, x: isize, y: isize, mouse_event: &MouseEvent) {
        if mouse_event.buttons.left() {
            for window in self.windows.iter_mut() {
                if window
                    .upgrade()
                    .unwrap()
                    .lock()
                    .rect
                    .detect_collision(&Rect::new(
                        self.mouse.width,
                        self.mouse.height,
                        self.mouse.x,
                        self.mouse.y,
                    ))
                {
                    let window_rect = window.upgrade().unwrap().lock().rect;
                    let mut new_pos_x = window_rect.x + x;
                    let mut new_pos_y = window_rect.y - y;

                    //handle left
                    if (new_pos_x + (window_rect.width as isize - 20)) < 0 {
                        new_pos_x = window_rect.x;
                    }

                    //handle right
                    if (new_pos_x + 20) > self.v_framebuffer.width as isize {
                        new_pos_x = window_rect.x;
                    }

                    //handle top
                    if new_pos_y <= 0 {
                        new_pos_y = window_rect.y;
                    }

                    if new_pos_y + 20 > self.v_framebuffer.height as isize {
                        new_pos_y = window_rect.y;
                    }

                    window.upgrade().unwrap().lock().rect.x = new_pos_x;
                    window.upgrade().unwrap().lock().rect.y = new_pos_y;
                }
            }
        } else if mouse_event.buttons.right() {
            let pos_x = self.mouse.x;
            let pos_y = self.mouse.y;

            for window in self.windows.iter_mut() {
                if window
                    .upgrade()
                    .unwrap()
                    .lock()
                    .rect
                    .detect_collision(&Rect::new(
                        self.mouse.width,
                        self.mouse.height,
                        self.mouse.x,
                        self.mouse.y,
                    ))
                {
                    window.upgrade().unwrap().lock().rect.width += x as usize;
                    window.upgrade().unwrap().lock().rect.height -= y as usize;
                    window.upgrade().unwrap().lock().resized = true;
                }
            }
        }
    }

    #[inline]
    fn render(&mut self) {
        self.p_framebuffer
            .buffer
            .copy_from_slice(&self.v_framebuffer.buffer);
    }
}

pub struct Window {
    rect: Rect,
    pub frame_buffer: FrameBuffer,
    resized: bool,
}

impl Window {
    fn new(rect: Rect, frame_buffer: FrameBuffer) -> Window {
        Window {
            rect,
            frame_buffer,
            resized: false,
        }
    }

    pub fn blank(&mut self) {
        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }

    pub fn blank_with_color(&mut self, rect: &Rect, col: u32) {
        let start_x = rect.x;
        let end_x = start_x + rect.width as isize;

        let start_y = rect.y;
        let end_y = start_y + rect.height as isize;

        for y in rect.x..rect.height as isize {
            for x in rect.y..rect.width as isize {
                self.draw_something(x as isize, y as isize, col);
            }
        }
    }

    pub fn draw_absolute(&mut self, x: isize, y: isize, col: u32) {
        if x <= self.rect.width as isize && y <= self.rect.height as isize {
            self.draw_something(x, y, col);
        }
    }

    pub fn draw_relative(&mut self, x: isize, y: isize, col: u32) {
        let x = x - self.rect.x;
        let y = y - self.rect.y;

        self.draw_something(x, y, col);
    }

    // TODO: Change the name
    fn draw_something(&mut self, x: isize, y: isize, col: u32) {
        if x >= 0 && x <= self.rect.width as isize && y >= 0 && y <= self.rect.height as isize {
            self.frame_buffer.buffer[(self.frame_buffer.width * y as usize) + x as usize] = col;
        }
    }

    pub fn draw_rectangle(&mut self, col: u32) {
        // TODO: This should be somewhere else and it should be a function
        if self.resized {
            self.resize_framebuffer();
            self.resized = false;
        }
        for y in 0..self.rect.height {
            for x in 0..self.rect.width {
                self.draw_something(x as isize, y as isize, col);
            }
        }
    }
    pub fn set_position(&mut self, x: isize, y: isize) {
        self.rect.x = x;
        self.rect.y = y;
    }

    fn resize_framebuffer(&mut self) {
        self.frame_buffer = FrameBuffer::new(self.rect.width, self.rect.height, None).unwrap();
    }
}

fn port_loop(
    (key_consumer, mouse_consumer): (Queue<Event>, Queue<Event>),
) -> Result<(), &'static str> {
    let window_manager = WINDOW_MANAGER.get().unwrap();
    let window_2 = WindowManager::new_window(&Rect::new(400, 400, 0, 0));
    let text = TextDisplay {
        width: 400,
        height: 400,
        next_col: 1,
        next_line: 1,
        text: "asdasd".to_string(),
        fg_color: 0xFFFFFF,
        bg_color: 0x0F0FFF,
    };
    let mut app = App::new(window_2, text);
    let hpet = get_hpet();
    let mut start = hpet
        .as_ref()
        .ok_or("couldn't get HPET timer")?
        .get_counter();
    let hpet_freq = hpet.as_ref().ok_or("ss")?.counter_period_femtoseconds() as u64;

    loop {
        let mut end = hpet
            .as_ref()
            .ok_or("couldn't get HPET timer")?
            .get_counter();
        let mut diff = (end - start) * hpet_freq / 1_000_000_000_00;
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
                        window_manager.lock().update_mouse_position(x, y);
                        window_manager.lock().drag_windows(x, y, &mouse_event);
                    }
                }
                _ => (),
            }
        }

        if diff >= 0 {
            app.draw();
            window_manager.lock().update();
            window_manager.lock().render();

            start = hpet.as_ref().unwrap().get_counter();
        }
    }
    Ok(())
}
