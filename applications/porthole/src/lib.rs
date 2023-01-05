#![no_std]
#![feature(slice_ptr_get)]
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
use core::marker::PhantomData;
use core::ops::Add;
use core::slice::IterMut;

use alloc::format;
use alloc::sync::{Arc, Weak};
use log::{debug, info};
use spin::{Mutex, MutexGuard, Once};

use event_types::Event;
use keycodes_ascii::{KeyAction, KeyEvent, Keycode};
use mpmc::Queue;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_BASIC};
use hpet::get_hpet;
use memory::{BorrowedSliceMappedPages, Mutable, PhysicalAddress, PteFlags, PteFlagsArch};
use mouse_data::MouseEvent;
pub static WINDOW_MANAGER: Once<Mutex<WindowManager>> = Once::new();
static TITLE_BAR_HEIGHT: usize = 20;
static SCREEN_WIDTH: usize = 1024;
static SCREEN_HEIGHT: usize = 768;

// We could do some fancy stuff with this like a trait, that can convert rgb to hex
// hex to rgb hsl etc, but for now it feels like bikeshedding
type Color = u32;
static DEFAULT_BORDER_COLOR: Color = 0x141414;
static DEFAULT_TEXT_COLOR: Color = 0xFBF1C7;
static DEFAULT_WINDOW_COLOR: Color = 0x3C3836;

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
        self.window.lock().draw_rectangle(DEFAULT_WINDOW_COLOR);
        let rect = self.window.lock().rect;
        display_window_title(
            &mut self.window.lock(),
            DEFAULT_TEXT_COLOR,
            DEFAULT_BORDER_COLOR,
        );
        print_string(
            &mut self.window.lock(),
            rect.width,
            rect.height,
            &RelativePos::new(0, 0),
            &self.text.text,
            DEFAULT_TEXT_COLOR,
            DEFAULT_WINDOW_COLOR,
            0,
            1,
        )
    }
}

pub fn display_window_title(window: &mut Window, fg_color: Color, bg_color: Color) {
    if window.title.is_some() {
        let title = window.title.as_mut().unwrap().clone();
        let slice = title.as_str();
        let border = window.title_border();
        let title_pos = window.title_pos(&slice.len());
        print_string(window, border.width,border.height, &title_pos, slice, fg_color, bg_color, 0, 0);
    }
}

pub fn print_string(
    window: &mut Window,
    width: usize,
    height: usize,
    pos: &RelativePos,
    slice: &str,
    fg_color: Color,
    bg_color: Color,
    column: usize,
    line: usize,
) {
    let slice = slice.as_bytes();
    let start_x = pos.x + (column as u32 * CHARACTER_WIDTH as u32);
    let start_y = pos.y + (line as u32 * CHARACTER_HEIGHT as u32);

    let mut x_index = 0;
    let mut row_controller = 0;
    let mut char_index = 0;
    let mut char_color_on_x_axis = x_index;
    loop {
        let x = start_x + x_index as u32;
        let y = start_y + row_controller as u32;
        if x_index % CHARACTER_WIDTH == 0 {
            char_color_on_x_axis = 0;
        }
        let color = if char_color_on_x_axis >= 1 {
            let index = char_color_on_x_axis - 1;
            let char_font = FONT_BASIC[slice[char_index] as usize][row_controller];
            char_color_on_x_axis += 1;
            if get_bit(char_font, index) != 0 {
                fg_color
            } else {
                bg_color
            }
        } else {
            char_color_on_x_axis += 1;
            bg_color
        };
        window.draw_unchecked(&RelativePos::new(x, y), color);

        x_index += 1;
        if x_index == CHARACTER_WIDTH
            || x_index % CHARACTER_WIDTH == 0
            || start_x + x_index as u32 == width as u32
        {
            if slice.len() >= 1 && char_index < slice.len() - 1 {
                char_index += 1;
            }

            if x_index >= CHARACTER_WIDTH * slice.len()
                && x_index % (CHARACTER_WIDTH * slice.len()) == 0
            {
                row_controller += 1;
                char_index = 0;
                x_index = 0;
            }

            if row_controller == CHARACTER_HEIGHT
                || start_y + row_controller as u32 == height as u32
            {
                break;
            }
        }
    }
}
fn get_bit(char_font: u8, i: usize) -> u8 {
    char_font & (0x80 >> i)
}

// TODO: Implement proper `types` for width height etc
pub struct TextDisplay {
    width: usize,
    height: usize,
    next_col: usize,
    next_line: usize,
    text: String,
    fg_color: Color,
    bg_color: Color,
}

impl TextDisplay {
    pub fn new(
        width: usize,
        height: usize,
        next_col: usize,
        next_line: usize,
        text: String,
        fg_color: Color,
        bg_color: Color,
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

    pub fn append_char(&mut self, char: char) {
        self.text.push(char);
    }
}

pub struct Dimensions {
    pub width: usize,
    pub height: usize,
}

impl Dimensions {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }
}

/// Position that is relative to a `Window`
#[derive(Clone, Copy)]
pub struct RelativePos {
    pub x: u32,
    pub y: u32,
}

impl RelativePos {
    pub fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }

    pub fn to_1d_pos(&self,target_stride: u32) -> usize{
        ((target_stride * self.y) + self.x) as usize
    }
}

/// Position that is relative to the screen 
pub struct ScreenPos {
    pub x: i32,
    pub y: i32,
}

impl ScreenPos {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn to_1d_pos(&self) -> usize{
        ((SCREEN_WIDTH as i32 * self.y) + self.x) as usize
    }
}

impl Add for ScreenPos {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl Add<Rect> for ScreenPos {
    type Output = Self;

    fn add(self, other: Rect) -> Self {
        Self {
            x: self.x + other.x as i32,
            y: self.y + other.y as i32,
        }
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

    fn x_plus_width(&self) -> isize {
        self.x + self.width as isize
    }

    fn y_plus_height(&self) -> isize {
        self.y + self.height as isize
    }

    fn detect_collision(&self, other: &Rect) -> bool {
        if self.x < other.x_plus_width()
            && self.x_plus_width() > other.x
            && self.y < other.y_plus_height()
            && self.y_plus_height() > other.y
        {
            true
        } else {
            false
        }
    }

    /// Creates a new `Rect` from visible parts of itself.
    pub fn visible_rect(&self) -> Rect {
        let mut x = self.x;
        let y = self.y;
        let mut width = self.width as isize;
        let mut height = self.height as isize;
        if self.x < 0 {
            x = 0;
            width = self.x_plus_width();
        } else if self.x_plus_width() > SCREEN_WIDTH as isize {
            x = self.x;
            let gap = (self.x + self.width as isize) - SCREEN_WIDTH as isize;
            width = self.width as isize - gap;
        }
        if self.y_plus_height() > SCREEN_HEIGHT as isize {
            let gap = (self.y + self.height as isize) - SCREEN_HEIGHT as isize;
            height = self.height as isize - gap;
        }
        let visible_rect = Rect::new(width as usize, height as usize, x, y);
        visible_rect
    }
}

pub struct VirtualFrameBuffer {
    width: usize,
    height: usize,
    buffer: BorrowedSliceMappedPages<u32, Mutable>,
}

impl VirtualFrameBuffer {
    pub fn new(width: usize, height: usize) -> Result<VirtualFrameBuffer, &'static str> {
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let size = width * height * core::mem::size_of::<u32>();
        let pages = memory::allocate_pages_by_bytes(size)
            .ok_or("could not allocate pages for a new framebuffer")?;
        let mapped_buffer = kernel_mmi_ref
            .lock()
            .page_table
            .map_allocated_pages(pages, PteFlags::new().valid(true).writable(true))?;
        Ok(VirtualFrameBuffer {
            width,
            height,
            buffer: mapped_buffer
                .into_borrowed_slice_mut(0, width * height)
                .map_err(|(_mp, s)| s)?,
        })
    }

    fn copy_windows_into_main_vbuffer(&mut self, window: &mut MutexGuard<Window>) {
        let window_screen = window.rect.visible_rect();
        let window_stride = window.frame_buffer.width as usize;

        // FIXME: Handle errors with error types
        let screen_rows = FramebufferRowChunks::new(&mut self.buffer, window_screen, self.width).unwrap();
        // To handle rendering when the window is partially outside the screen we use relative version of visible rect
        let relative_visible_rect = window.relative_visible_rect();

        let window_rows = FramebufferRowChunks::new(
            &mut window.frame_buffer.buffer,
            relative_visible_rect,
            window_stride,
        )
        .unwrap();
        for (screen_row, window_row) in screen_rows.zip(window_rows) {
            screen_row.copy_from_slice(window_row);
        }

    }

    fn draw_unchecked(&mut self, x: isize, y: isize, col: Color) {
        unsafe {
            let index = (self.width * y as usize) + x as usize;
            let pixel = self.buffer.get_unchecked_mut(index);
            *pixel = col;
        }
    }
    pub fn blank(&mut self) {
        for pixel in self.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }
}

/// Physical framebuffer we use for final rendering to the screen.
pub struct PhysicalFrameBuffer {
    width: usize,
    height: usize,
    buffer: BorrowedSliceMappedPages<u32, Mutable>,
}
impl PhysicalFrameBuffer {
    fn init_front_buffer() -> Result<PhysicalFrameBuffer, &'static str> {
        let graphic_info = multicore_bringup::GRAPHIC_INFO.lock();
        if graphic_info.physical_address() == 0 {
            return Err("wrong physical address for porthole");
        }
        let vesa_display_phys_start =
            PhysicalAddress::new(graphic_info.physical_address() as usize).ok_or("Invalid address");
        let buffer_width = graphic_info.width() as usize;
        let buffer_height = graphic_info.height() as usize;

        let framebuffer = PhysicalFrameBuffer::new(
            buffer_width,
            buffer_height,
            vesa_display_phys_start.unwrap(),
        )?;
        Ok(framebuffer)
    }

    pub fn new(
        width: usize,
        height: usize,
        physical_address: PhysicalAddress,
    ) -> Result<PhysicalFrameBuffer, &'static str> {
        let kernel_mmi_ref =
            memory::get_kernel_mmi_ref().ok_or("KERNEL_MMI was not yet initialized!")?;
        let size = width * height * core::mem::size_of::<u32>();
        let pages = memory::allocate_pages_by_bytes(size)
            .ok_or("could not allocate pages for a new framebuffer")?;

        let mapped_framebuffer = {
            let mut flags: PteFlagsArch = PteFlags::new().valid(true).writable(true).into();

            #[cfg(target_arch = "x86_64")]
            {
                let use_pat = page_attribute_table::init().is_ok();
                if use_pat {
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

            let frames = memory::allocate_frames_by_bytes_at(physical_address, size)
                .map_err(|_e| "Couldn't allocate frames for the final framebuffer")?;
            let fb_mp = kernel_mmi_ref
                .lock()
                .page_table
                .map_allocated_pages_to(pages, frames, flags)?;
            debug!("Mapped real physical framebuffer: {fb_mp:?}");
            fb_mp
        };
        Ok(PhysicalFrameBuffer {
            width,
            height,
            buffer: mapped_framebuffer
                .into_borrowed_slice_mut(0, width * height)
                .map_err(|(_mp, s)| s)?,
        })
    }
}

struct FramebufferRowChunks<'a, T: 'a> {
    fb: *mut [T],
    rect: Rect,
    stride: usize,
    row_index_beg: usize,
    row_index_end: usize,
    current_column: usize,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T: 'a> FramebufferRowChunks<'a, T> {
    #[inline]
    pub fn new(slice: &'a mut [T], rect: Rect, stride: usize) -> Option<Self> {
        if rect.width <= stride {
            let current_column = rect.y as usize;
            let row_index_beg = (stride * current_column) + rect.x as usize;
            let row_index_end = (stride * current_column) + rect.x_plus_width() as usize;
            Some(Self {
                fb: slice,
                rect,
                stride,
                row_index_beg,
                row_index_end,
                current_column,
                _marker: PhantomData,
            })
        } else {
            None
        }
    }

    fn calculate_next_row(&mut self) {
        self.row_index_beg = (self.stride * self.current_column) + self.rect.x as usize;
        self.row_index_end = (self.stride * self.current_column) + self.rect.x_plus_width() as usize;
    }
}

impl<'a, T> Iterator for FramebufferRowChunks<'a, T> {
    type Item = &'a mut [T];

    fn next(&mut self) -> Option<&'a mut [T]> {
        if self.current_column < self.rect.y_plus_height() as usize {
            let chunk = unsafe {
                self.fb
                    .get_unchecked_mut(self.row_index_beg..self.row_index_end)
            };
            self.current_column += 1;
            self.calculate_next_row();
            let chunk = { unsafe { &mut *chunk } };
            Some(chunk)
        } else {
            None
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

#[derive(PartialEq, Eq)]
pub enum Holding {
    Background,
    Nothing,
    Window(usize),
}

pub struct WindowManager {
    windows: Vec<Weak<Mutex<Window>>>,
    window_rendering_order: Vec<usize>,
    v_framebuffer: VirtualFrameBuffer,
    p_framebuffer: PhysicalFrameBuffer,
    pub mouse: Rect,
    mouse_holding: Holding,
}

impl WindowManager {
    fn init() {
        let p_framebuffer = PhysicalFrameBuffer::init_front_buffer().unwrap();
        let v_framebuffer =
            VirtualFrameBuffer::new(p_framebuffer.width, p_framebuffer.height).unwrap();
        // FIXME: Don't use magic numbers
        let mouse = Rect::new(11, 18, 200, 200);

        let window_manager = WindowManager {
            windows: Vec::new(),
            window_rendering_order: Vec::new(),
            v_framebuffer,
            p_framebuffer,
            mouse,
            mouse_holding: Holding::Nothing,
        };
        WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));
    }

    fn new_window(rect: &Rect, title: Option<String>) -> Arc<Mutex<Window>> {
        let mut manager = WINDOW_MANAGER.get().unwrap().lock();
        let len = manager.windows.len();

        manager.window_rendering_order.push(len);
        let window = Window::new(
            *rect,
            VirtualFrameBuffer::new(rect.width, rect.height).unwrap(),
            title,
        );
        let arc_window = Arc::new(Mutex::new(window));
        manager.windows.push(Arc::downgrade(&arc_window.clone()));
        arc_window
    }

    fn draw_windows(&mut self) {
        for order in self.window_rendering_order.iter() {
            self.v_framebuffer
                .copy_windows_into_main_vbuffer(&mut self.windows[*order].upgrade().unwrap().lock());
        }
        for window in self.windows.iter() {
            window.upgrade().unwrap().lock().blank();
        }
    }

    // TODO: Stop indexing mouse image create iterator for it, also draw the thing with iterators
    fn draw_mouse(&mut self) {
        let bounding_box = self.mouse.visible_rect();
        for y in bounding_box.y..bounding_box.y + bounding_box.height as isize {
            for x in bounding_box.x..bounding_box.x + bounding_box.width as isize {
                let color = MOUSE_POINTER_IMAGE[(x - bounding_box.x) as usize]
                    [(y - bounding_box.y) as usize];
                if color != 0xFF0000 {
                    self.v_framebuffer.draw_unchecked(x, y, color);
                }
            }
        }
    }

    pub fn set_mouse_pos(&mut self, screen_pos: &ScreenPos) {
        self.mouse.x = screen_pos.x as isize;
        self.mouse.y = screen_pos.y as isize;
    }

    fn update(&mut self) {
        self.v_framebuffer.blank();
        self.draw_windows();
        self.draw_mouse();
    }

    // TODO: Remove magic numbers
    fn update_mouse_position(&mut self, screen_pos: ScreenPos) {
        let mut new_pos = screen_pos + self.mouse;

        // handle left
        new_pos.x = core::cmp::max(new_pos.x, 0);
        // handle right
        new_pos.x = core::cmp::min(new_pos.x, self.v_framebuffer.width as i32 - 3);

        // handle top
        new_pos.y = core::cmp::max(new_pos.y, 0);
        // handle bottom
        new_pos.y = core::cmp::min(new_pos.y, self.v_framebuffer.height as i32 - 3);

        self.set_mouse_pos(&new_pos);
    }

    fn drag_windows(&mut self, screen_pos: ScreenPos, mouse_event: &MouseEvent) {
        if mouse_event.buttons.left() {
            match self.mouse_holding {
                Holding::Background => todo!(),
                Holding::Nothing => {
                    let rendering_o = self.window_rendering_order.clone();
                    for &i in rendering_o.iter().rev() {
                        let window = &mut self.windows[i];
                        if window
                            .upgrade()
                            .unwrap()
                            .lock()
                            .dynamic_title_border_pos()
                            .detect_collision(&self.mouse)
                        {
                            if i != *self.window_rendering_order.last().unwrap() {
                                let wind_index = self
                                    .window_rendering_order
                                    .iter()
                                    .position(|ii| ii == &i)
                                    .unwrap();
                                self.window_rendering_order.remove(wind_index);
                                self.window_rendering_order.push(i);
                            }
                            // FIXME: Don't hold a window if its behind another window
                            self.mouse_holding = Holding::Window(i);
                            break;
                        }
                        self.mouse_holding = Holding::Nothing;
                    }
                }
                // TODO: Fix the bug that allows you to move the window while mouse position is still
                Holding::Window(i) => {
                    let window = &mut self.windows[i];
                    let window_rect = window.upgrade().unwrap().lock().rect;
                    let mut new_pos = screen_pos + window_rect;

                    //handle left
                    if (new_pos.x + (window_rect.width as i32 - 20)) < 0 {
                        new_pos.x = window_rect.x as i32;
                    }

                    //handle right
                    if (new_pos.x + 20) > self.v_framebuffer.width as i32 {
                        new_pos.x = window_rect.x as i32;
                    }

                    //handle top
                    if new_pos.y < 0 {
                        new_pos.y = window_rect.y as i32;
                    }

                    // handle bottom
                    if new_pos.y + 20 > self.v_framebuffer.height as i32 {
                        new_pos.y = window_rect.y as i32;
                    }

                    window.upgrade().unwrap().lock().set_screen_pos(&new_pos);
                }
            }
        // FIXME: Resizing is broken if windows are on top of each other
        } else if mouse_event.buttons.right() {
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
                    window
                        .upgrade()
                        .unwrap()
                        .lock()
                        .resize_window(screen_pos.x, screen_pos.y);
                    window.upgrade().unwrap().lock().reset_drawable_area();
                    window
                        .upgrade()
                        .unwrap()
                        .lock()
                        .reset_title_pos_and_border();
                    window.upgrade().unwrap().lock().resized = true;
                }
            }
        }
        if !mouse_event.buttons.left() {
            self.mouse_holding = Holding::Nothing;
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
    pub frame_buffer: VirtualFrameBuffer,
    resized: bool,
    title: Option<String>,
    title_border: Option<Rect>,
    title_pos: Option<RelativePos>,
    drawable_area: Option<Rect>,
}

impl Window {
    fn new(rect: Rect, frame_buffer: VirtualFrameBuffer, title: Option<String>) -> Window {
        Window {
            rect,
            frame_buffer,
            resized: false,
            title,
            title_border: None,
            title_pos: None,
            drawable_area: None,
        }
    }

    pub fn width(&self) -> usize {
        self.rect.width
    }

    pub fn height(&self) -> usize {
        self.rect.height
    }

    pub fn screen_pos(&self) -> ScreenPos {
        let screen_pos = ScreenPos::new(self.rect.x as i32, self.rect.y as i32);
        screen_pos
    }

    pub fn set_screen_pos(&mut self, screen_pos: &ScreenPos) {
        self.rect.x = screen_pos.x as isize;
        self.rect.y = screen_pos.y as isize;
    }

    pub fn blank(&mut self) {
        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }

    pub fn resize_window(&mut self, width: i32, height: i32) {
        let new_width = self.width() + width as usize;
        let new_height = self.height() + height as usize;
        if new_width > 50 && new_height > 50 {
            self.rect.width = new_width;
            self.rect.height = new_height;
        }
    }

    pub fn reset_drawable_area(&mut self) {
        self.drawable_area = None;
    }

    pub fn reset_title_pos_and_border(&mut self) {
        self.title_border = None;
        self.title_pos = None;
    }

    /// Returns Window's border area width and height with 0 as position
    pub fn title_border(&mut self) -> Rect {
        let border =
            self.title_border
                .get_or_insert(Rect::new(self.rect.width, TITLE_BAR_HEIGHT, 0, 0));
        *border
    }

    /// Return's title border's position in screen coordinates
    pub fn dynamic_title_border_pos(&self) -> Rect {
        let mut rect = self.rect;
        rect.height = TITLE_BAR_HEIGHT;
        rect
    }

    /// Return's drawable area
    pub fn drawable_area(&mut self) -> Rect {
        let border = self.title_border();
        let drawable_area = self.drawable_area.get_or_insert({
            let x = 0;
            let y = border.height;
            let width = border.width;
            let height = self.rect.height - y;
            let drawable_area = Rect::new(width, height, x, y as isize);
            drawable_area
        });
        *drawable_area
    }

    pub fn title_pos(&mut self, slice_len: &usize) -> RelativePos {
        let border = self.title_border();
        let relative_pos = self.title_pos.get_or_insert({
            let pos = (border.width - (slice_len * CHARACTER_WIDTH)) / 2;
            let relative_pos = RelativePos::new(pos as u32, 0);
            relative_pos
        });
        *relative_pos
    }

    pub fn draw_title_border(&mut self) {
        let border = self.title_border();
        if let Some(rows) = FramebufferRowChunks::new(
            &mut self.frame_buffer.buffer,
            border,
            self.frame_buffer.width,
        ) {
            for row in rows {
                for pixel in row {
                    *pixel = DEFAULT_BORDER_COLOR;
                }
            }
        }
    }

    // TODO: look into this
    fn draw_unchecked(&mut self, relative_pos: &RelativePos, col: Color) {
        let x = relative_pos.x;
        let y = relative_pos.y;
        unsafe {
            let index = (self.frame_buffer.width * y as usize) + x as usize;
            let pixel = self.frame_buffer.buffer.get_unchecked_mut(index);
            *pixel = col;
        }
    }

    fn should_resize_framebuffer(&mut self) {
        if self.resized {
            self.resize_framebuffer();
            self.resized = false;
        }
    }

    /// Draws the rectangular shape representing the `Window`
    pub fn draw_rectangle(&mut self, col: Color) {
        self.should_resize_framebuffer();

        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = col;
        }
        self.draw_title_border();
    }

    /// Resizes framebuffer after to Window's width and height
    fn resize_framebuffer(&mut self) {
        self.frame_buffer = VirtualFrameBuffer::new(self.rect.width, self.rect.height).unwrap();
    }

    /// Returns visible part of self's `rect` with relative bounds applied
    /// e.g if visible rect is `Rect{width: 358, height: 400, x: 0, y: 0}`
    /// this will return `Rect{width: 358, height: 400, x: 42, y: 0}`
    pub fn relative_visible_rect(&self) -> Rect {
        let mut bounding_box = self.rect.visible_rect();
        bounding_box.x = 0;
        if self.left_side_out() {
            bounding_box.x = (self.rect.width - bounding_box.width) as isize;
        }
        bounding_box.y = 0;
        bounding_box
    }

    pub fn left_side_out(&self) -> bool {
        self.rect.x < 0
    }

    pub fn right_side_out(&self) -> bool {
        self.rect.x + self.rect.width as isize > SCREEN_WIDTH as isize
    }

    pub fn bottom_side_out(&self) -> bool {
        self.rect.y + self.rect.height as isize > SCREEN_HEIGHT as isize
    }
}

fn port_loop(
    (key_consumer, mouse_consumer): (Queue<Event>, Queue<Event>),
) -> Result<(), &'static str> {
    let window_manager = WINDOW_MANAGER.get().unwrap();
    let window_3 = WindowManager::new_window(&Rect::new(400, 200, 30, 100), None);
    let window_2 = WindowManager::new_window(&Rect::new(400, 400, 500, 20), Some(format!("Basic")));
    let text = TextDisplay {
        width: 400,
        height: 400,
        next_col: 1,
        next_line: 1,
        text: "Hello World".to_string(),
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
        let end = hpet
            .as_ref()
            .ok_or("couldn't get HPET timer")?
            .get_counter();
        let diff = (end - start) * hpet_freq / 1_000_000;
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
                        window_manager
                            .lock()
                            .update_mouse_position(ScreenPos::new(x as i32, -(y as i32)));
                        window_manager
                            .lock()
                            .drag_windows(ScreenPos::new(x as i32, -(y as i32)), &mouse_event);
                    }
                }
                _ => (),
            }
        }

        if diff >= 0 {
            app.draw();
            window_3.lock().draw_rectangle(DEFAULT_WINDOW_COLOR);
            window_manager.lock().update();
            window_manager.lock().render();

            start = hpet.as_ref().unwrap().get_counter();
        }
    }
    Ok(())
}
