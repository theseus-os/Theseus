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
use alloc::format;
use alloc::sync::Arc;
use core::marker::PhantomData;
use core::ops::{Add, Sub};
use log::{debug, info};
use spin::{Mutex, MutexGuard, Once};

use event_types::Event;
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
    text: TextDisplayInfo,
}

impl App {
    pub fn new(window: Arc<Mutex<Window>>, text: TextDisplayInfo) -> Self {
        Self { window, text }
    }
    pub fn draw(&mut self) -> Result<(), &'static str> {
        let mut window = self.window.lock();
        {
            window.draw_rectangle(DEFAULT_WINDOW_COLOR)?;
            window.display_window_title(DEFAULT_TEXT_COLOR, DEFAULT_BORDER_COLOR);
            print_string(
                &mut window,
                self.text.width,
                self.text.height,
                &self.text.pos,
                &self.text.text,
                self.text.fg_color,
                self.text.bg_color,
                self.text.next_col,
                self.text.next_line,
            );
        }
        Ok(())
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

pub struct TextDisplayInfo {
    width: usize,
    height: usize,
    pos: RelativePos,
    next_col: usize,
    next_line: usize,
    text: String,
    fg_color: Color,
    bg_color: Color,
}

impl TextDisplayInfo {
    pub fn new(
        width: usize,
        height: usize,
        pos: RelativePos,
        next_col: usize,
        next_line: usize,
        text: String,
        fg_color: Color,
        bg_color: Color,
    ) -> Self {
        Self {
            width,
            height,
            pos,
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

    pub fn to_1d_pos(&self, target_stride: u32) -> usize {
        ((target_stride * self.y) + self.x) as usize
    }
}

/// Position that is relative to the screen
#[derive(Debug, Clone, Copy)]
pub struct ScreenPos {
    pub x: i32,
    pub y: i32,
}

impl ScreenPos {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn to_1d_pos(&self) -> usize {
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

impl Sub for ScreenPos {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
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

    pub fn to_screen_pos(&self) -> ScreenPos {
        ScreenPos {
            x: self.x as i32,
            y: self.y as i32,
        }
    }

    fn x_plus_width(&self) -> isize {
        self.x + self.width as isize
    }

    fn y_plus_height(&self) -> isize {
        self.y + self.height as isize
    }

    fn detect_collision(&self, other: &Rect) -> bool {
        self.x < other.x_plus_width()
            && self.x_plus_width() > other.x
            && self.y < other.y_plus_height()
            && self.y_plus_height() > other.y
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

    fn copy_window_into_main_vbuffer(&mut self, window: &mut MutexGuard<Window>) {
        let window_screen = window.rect.visible_rect();
        let window_stride = window.frame_buffer.width as usize;

        if let Some(screen_rows) =
            FramebufferRowChunks::new(&mut self.buffer, window_screen, self.width)
        {
            // To handle rendering when the window is partially outside the screen we use relative version of visible rect
            let relative_visible_rect = window.relative_visible_rect();
            if let Some(window_rows) = FramebufferRowChunks::new(
                &mut window.frame_buffer.buffer,
                relative_visible_rect,
                window_stride,
            ) {
                for (screen_row, window_row) in screen_rows.zip(window_rows) {
                    screen_row.copy_from_slice(window_row);
                }
            }
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
    stride: usize,
    buffer: BorrowedSliceMappedPages<u32, Mutable>,
}
impl PhysicalFrameBuffer {
    fn init_front_buffer() -> Result<PhysicalFrameBuffer, &'static str> {
        let graphic_info =
            multicore_bringup::get_graphic_info().ok_or("Failed to get graphic info")?;
        if graphic_info.physical_address() == 0 {
            return Err("wrong physical address for porthole");
        }
        let vesa_display_phys_start =
            PhysicalAddress::new(graphic_info.physical_address() as usize)
                .ok_or("Invalid address")?;
        let buffer_width = graphic_info.width() as usize;
        let buffer_height = graphic_info.height() as usize;
        // We are assuming a pixel is 4 bytes big
        let stride = graphic_info.bytes_per_scanline() / 4;

        let framebuffer = PhysicalFrameBuffer::new(
            buffer_width,
            buffer_height,
            stride as usize,
            vesa_display_phys_start,
        )?;
        Ok(framebuffer)
    }

    pub fn new(
        width: usize,
        height: usize,
        stride: usize,
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
            stride,
            buffer: mapped_framebuffer
                .into_borrowed_slice_mut(0, width * height)
                .map_err(|(_mp, s)| s)?,
        })
    }
}

/// Our mouse image is [`MOUSE_POINTER_IMAGE`] column major 2D array
/// This type returns us row major, 1D vec of that image
pub struct MouseImageRowIterator<'a> {
    /// Mouse image [`MOUSE_POINTER_IMAGE`]
    mouse_image: &'a [[u32; 18]; 11],
    /// Rect of our mouse
    bounding_box: Rect,
    /// Since image is column major we will iterate will use
    /// individual columns to create a row, think of it as y axis
    current_column: usize,
    /// Used to traverse image in x axis
    current_row: usize,
}

impl<'a> MouseImageRowIterator<'a> {
    fn new(mouse_image: &'a [[u32; 18]; 11], bounding_box: Rect) -> Self {
        Self {
            mouse_image,
            bounding_box,
            current_column: 0,
            current_row: 0,
        }
    }
}

impl<'a> Iterator for MouseImageRowIterator<'a> {
    type Item = Vec<u32>;

    fn next(&mut self) -> Option<Vec<u32>> {
        // We start from MOUSE_POINTER_IMAGE[0][0], get the color on that index push it to our `row`
        // then move to MOUSE_POINTER_IMAGE[1][0] do the same thing
        // until we hit `bounding_box.width -1` then we reset our `current_column` to `0` and increase
        // our `current_row` by `1`
        if self.current_row < self.bounding_box.height - 1 {
            let mut row = Vec::new();
            while self.current_column < self.bounding_box.width {
                let color = unsafe {
                    self.mouse_image.get_unchecked(self.current_column)[self.current_row]
                };
                row.push(color);
                self.current_column += 1;
                if self.current_column == self.bounding_box.width - 1 {
                    self.current_column = 0;
                    break;
                }
            }
            self.current_row += 1;
            Some(row)
        } else {
            None
        }
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
        self.row_index_end =
            (self.stride * self.current_column) + self.rect.x_plus_width() as usize;
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

pub fn main(_args: Vec<String>) -> Result<isize, &'static str> {
    let mouse_consumer = Queue::with_capacity(100);
    let mouse_producer = mouse_consumer.clone();
    let key_consumer = Queue::with_capacity(100);
    let key_producer = mouse_consumer.clone();
    WindowManager::init()?;
    device_manager::init(key_producer, mouse_producer)
        .or(Err("Failed to initialize device manager"))?;

    let _task_ref = match spawn::new_task_builder(port_loop, (mouse_consumer, key_consumer))
        .name("port_loop".to_string())
        .spawn()
    {
        Ok(task_ref) => task_ref,
        Err(err) => {
            log::error!("{}", err);
            log::error!("failed to spawn shell");
            return Err("failed to spawn shell");
        }
    };

    task::get_my_current_task()
        .ok_or("Failed to get the current task")?
        .block()
        .or(Err("Failed to block the current task"))?;
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
pub struct WindowManager {
    windows: Vec<Arc<Mutex<Window>>>,
    window_rendering_order: Vec<usize>,
    v_framebuffer: VirtualFrameBuffer,
    p_framebuffer: PhysicalFrameBuffer,
    pub mouse: Rect,
    prev_mouse_pos: ScreenPos,
    mouse_holding: Holding,
    active_window_index: usize,
}

impl WindowManager {
    fn init() -> Result<(), &'static str> {
        let p_framebuffer = PhysicalFrameBuffer::init_front_buffer()?;
        let v_framebuffer = VirtualFrameBuffer::new(p_framebuffer.width, p_framebuffer.height)?;
        // FIXME: Don't use magic numbers
        let mouse = Rect::new(11, 18, 200, 200);

        let window_manager = WindowManager {
            windows: Vec::new(),
            window_rendering_order: Vec::new(),
            v_framebuffer,
            p_framebuffer,
            mouse,
            prev_mouse_pos: mouse.to_screen_pos(),
            mouse_holding: Holding::Nothing,
            active_window_index: 0,
        };
        WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));
        Ok(())
    }

    fn new_window(
        &mut self,
        rect: &Rect,
        title: Option<String>,
    ) -> Result<Arc<Mutex<Window>>, &'static str> {
        let len = self.windows.len();

        self.window_rendering_order.push(len);
        let window = Window::new(
            *rect,
            VirtualFrameBuffer::new(rect.width, rect.height)?,
            title,
        );
        let arc_window = Arc::new(Mutex::new(window));
        let returned_window = arc_window.clone();
        self.windows.push(arc_window);
        Ok(returned_window)
    }

    fn draw_windows(&mut self) {
        for order in self.window_rendering_order.iter() {
            self.v_framebuffer
                .copy_window_into_main_vbuffer(&mut self.windows[*order].lock());
        }
        for window in self.windows.iter() {
            window.lock().blank();
        }
    }

    fn draw_mouse(&mut self) {
        let bounding_box = self.mouse.visible_rect();

        let mouse_image = MouseImageRowIterator::new(&MOUSE_POINTER_IMAGE, bounding_box);
        let chunker =
            FramebufferRowChunks::new(&mut self.v_framebuffer.buffer, bounding_box, SCREEN_WIDTH)
                .unwrap();

        for (screen_row, mouse_image_row) in chunker.zip(mouse_image) {
            for (screen_pixel, mouse_pixel) in screen_row.iter_mut().zip(mouse_image_row.iter()) {
                if mouse_pixel != &0xFF0000 {
                    *screen_pixel = *mouse_pixel;
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

    fn calculate_next_mouse_pos(&self, curr_pos: ScreenPos, next_pos: ScreenPos) -> ScreenPos {
        let mut new_pos = next_pos + curr_pos;

        // handle left
        new_pos.x = core::cmp::max(new_pos.x, 0);
        // handle right
        new_pos.x = core::cmp::min(new_pos.x, self.v_framebuffer.width as i32 - 3);

        // handle top
        new_pos.y = core::cmp::max(new_pos.y, 0);
        // handle bottom
        new_pos.y = core::cmp::min(new_pos.y, self.v_framebuffer.height as i32 - 3);

        new_pos
    }

    // TODO: Remove magic numbers
    fn update_mouse_position(&mut self, screen_pos: ScreenPos) {
        self.prev_mouse_pos = self.mouse.to_screen_pos();
        let new_pos = self.calculate_next_mouse_pos(self.mouse.to_screen_pos(), screen_pos);

        self.set_mouse_pos(&new_pos);
    }

    fn drag_windows(&mut self, screen_pos: ScreenPos, mouse_event: &MouseEvent) {
        if mouse_event.buttons.left() {
            match self.mouse_holding {
                Holding::Background => {}
                Holding::Nothing => {
                    let rendering_o = self.window_rendering_order.clone();
                    for (window_index, pos) in rendering_o.iter().enumerate().rev() {
                        let window = &mut self.windows[window_index];
                        if window.lock().rect.detect_collision(&self.mouse) {
                            if window_index != self.active_window_index {
                                let last_one = self.window_rendering_order.len() - 1;
                                self.window_rendering_order.swap(last_one, *pos);
                            }
                            if window
                                .lock()
                                .dynamic_title_border_pos()
                                .detect_collision(&self.mouse)
                            {
                                self.active_window_index = window_index;
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
                    let next_mouse_pos = self.calculate_next_mouse_pos(prev_mouse_pos, screen_pos);
                    let window = &mut self.windows[i];
                    let window_rect = window.lock().rect;
                    let diff = next_mouse_pos - prev_mouse_pos;
                    let mut new_pos = diff + window_rect.to_screen_pos();

                    //handle left
                    if (new_pos.x + (window_rect.width as i32 - 20)) < 0 {
                        new_pos.x = -(window_rect.width as i32 - 20);
                    }

                    //handle right
                    if (new_pos.x + 20) > self.v_framebuffer.width as i32 {
                        new_pos.x = SCREEN_WIDTH as i32 - 20
                    }

                    //handle top
                    if new_pos.y < 0 {
                        new_pos.y = 0
                    }

                    // handle bottom
                    if new_pos.y + 20 > self.v_framebuffer.height as i32 {
                        new_pos.y = (SCREEN_HEIGHT - 20) as i32;
                    }

                    window.lock().set_screen_pos(&new_pos);
                }
            }
        } else if mouse_event.buttons.right() {
            let rendering_o = self.window_rendering_order.clone();
            for &i in rendering_o.iter().rev() {
                let window = &mut self.windows[i];
                if window.lock().rect.detect_collision(&Rect::new(
                    self.mouse.width,
                    self.mouse.height,
                    self.mouse.x,
                    self.mouse.y,
                )) {
                    window.lock().resize_window(screen_pos.x, screen_pos.y);
                    window.lock().reset_drawable_area();
                    window.lock().reset_title_pos_and_border();
                    window.lock().resized = true;
                    break;
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

    pub fn display_window_title(&mut self, fg_color: Color, bg_color: Color) {
        if let Some(title) = self.title.clone() {
            let slice = title.as_str();
            let border = self.title_border();
            let title_pos = self.title_pos(&slice.len());
            print_string(
                self,
                border.width,
                border.height,
                &title_pos,
                slice,
                fg_color,
                bg_color,
                0,
                0,
            );
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

    fn should_resize_framebuffer(&mut self) -> Result<(), &'static str> {
        if self.resized {
            self.resize_framebuffer()?;
            self.resized = false;
        }
        Ok(())
    }

    /// Draws the rectangular shape representing the `Window`
    pub fn draw_rectangle(&mut self, col: Color) -> Result<(), &'static str> {
        self.should_resize_framebuffer()?;

        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = col;
        }
        self.draw_title_border();
        Ok(())
    }

    /// Resizes framebuffer after to Window's width and height
    fn resize_framebuffer(&mut self) -> Result<(), &'static str> {
        self.frame_buffer = VirtualFrameBuffer::new(self.rect.width, self.rect.height).or(Err(
            "Unable to resize framebuffer to current width and height",
        ))?;
        Ok(())
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
    let window_manager = WINDOW_MANAGER.get().ok_or("Unable to get WindowManager")?;
    let window_2 = window_manager
        .lock()
        .new_window(&Rect::new(400, 400, 500, 20), Some(format!("Basic")))?;
    let text = TextDisplayInfo {
        width: 400,
        height: 400,
        pos: RelativePos::new(0, 0),
        next_col: 1,
        next_line: 1,
        text: "Hello World".to_string(),
        fg_color: DEFAULT_TEXT_COLOR,
        bg_color: DEFAULT_BORDER_COLOR,
    };
    // let window_3 = window_manager.lock().new_window(&Rect::new(100, 100, 0, 0), Some(format!("window 3")))?;
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
                    }
                    window_manager
                        .lock()
                        .drag_windows(ScreenPos::new(x as i32, -(y as i32)), &mouse_event);
                }
                _ => (),
            }
        }

        if diff > 0 {
            app.draw()?;
            window_manager.lock().update();
            window_manager.lock().render();

            start = hpet.as_ref().ok_or("Unable to get timer")?.get_counter();
        }
    }
    Ok(())
}
