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
use core::ops::Add;

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
            &rect.as_dimension(),
            &RelativePos::new(0, 0),
            &self.text.text,
            DEFAULT_TEXT_COLOR,
            DEFAULT_WINDOW_COLOR,
            0,
            1,
        )
    }
}

pub fn display_window_title(window: &mut MutexGuard<Window>, fg_color: Color, bg_color: Color) {
    if let Some(title) = window.title.clone() {
        let slice = title.as_str();
        let border = window.return_title_border().as_dimension();
        let title_pos = window.return_title_pos(&slice.len());
        print_string(window, &border, &title_pos, slice, fg_color, bg_color, 0, 0);
    }
}

pub fn print_string(
    window: &mut MutexGuard<Window>,
    dimensions: &Dimensions,
    pos: &RelativePos,
    slice: &str,
    fg_color: Color,
    bg_color: Color,
    column: usize,
    line: usize,
) {
    let slice = slice.as_bytes();
    let relative_x = pos.x;
    let relative_y = pos.y;
    let mut curr_column = column;
    let mut curr_line = line;
    let start_x = relative_x + (curr_column as u32 * CHARACTER_WIDTH as u32);
    let start_y = relative_y + (curr_line as u32 * CHARACTER_HEIGHT as u32);
    let off_set_x = 0;
    let off_set_y = 0;

    let mut j = off_set_x;
    let mut i = off_set_y;
    let mut z = 0;
    let mut index_j = j;
    loop {
        let x = start_x + j as u32;
        let y = start_y + i as u32;
        if j % CHARACTER_WIDTH == 0 {
            index_j = 0;
        }
        let color = if index_j >= 1 {
            let index = index_j - 1;
            let char_font = FONT_BASIC[slice[z] as usize][i];
            index_j += 1;
            if get_bit(char_font, index) != 0 {
                fg_color
            } else {
                bg_color
            }
        } else {
            index_j += 1;
            bg_color
        };
        window.draw_unchecked(&RelativePos::new(x, y), color);

        j += 1;
        if j == CHARACTER_WIDTH
            || j % CHARACTER_WIDTH == 0
            || start_x + j as u32 == dimensions.width as u32
        {
            if slice.len() >= 1 && z < slice.len() - 1 {
                z += 1;
            }

            if j >= CHARACTER_WIDTH * slice.len() && j % (CHARACTER_WIDTH * slice.len()) == 0 {
                i += 1;
                z = 0;
                j = off_set_x;
            }

            if i == CHARACTER_HEIGHT || start_y + i as u32 == dimensions.height as u32 {
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

pub struct RelativePos {
    pub x: u32,
    pub y: u32,
}

impl RelativePos {
    pub fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }
}

pub struct ScreenPos {
    pub x: i32,
    pub y: i32,
}

impl ScreenPos {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
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

    fn as_dimension(&self) -> Dimensions {
        Dimensions {
            width: self.width,
            height: self.height,
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

    pub fn on_screen_window(&self, screen_width: isize, screen_height: isize) -> Rect {
        let mut start_x = self.x;
        let start_y = self.y;
        let mut end_x = self.width as isize;
        let mut end_y = self.height as isize;
        if self.x < 0 {
            start_x = 0;
            end_x = self.x + self.width as isize;
        } else if self.x + self.width as isize > screen_width as isize {
            start_x = self.x;
            let gap = (self.x + self.width as isize) - screen_width as isize;
            end_x = self.width as isize - gap;
        }
        if self.y + self.height as isize > screen_height {
            let gap = (self.y + self.height as isize) - screen_height;
            end_y = self.height as isize - gap;
        }
        let f = Rect::new(end_x as usize, end_y as usize, start_x, start_y);
        f
    }
}

/// FrameBuffer with no actual physical memory mapped,
/// used for Window and WindowManager's backbuffer.
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

    fn copy_window_from_iterators(&mut self, window: &mut MutexGuard<Window>) {
        let window_screen = window.on_screen_window(self.width as isize, self.height as isize);

        let w_it = window.return_framebuffer_iterator();
        let f = buffer_indexer(&mut self.buffer, self.width, window_screen);

        for (w_color, pixel) in w_it.zip(f) {
            *pixel = *w_color;
        }
    }

    fn copy_window_only(&mut self, window: &mut MutexGuard<Window>) {
        /*
        (ouz-a):I feel like(because at this point it's very hard to benchmark performance as our resolution is small)
                this version is faster than iterator version below, could be improved with a lot of work, but still would
                require `unsafe`.

        let bounding_box = window.on_screen_window(self.width as isize, self.height as isize);
        for y in 0..bounding_box.height {
                let x = 0;
                let y = y as isize;
                let real_x = x + bounding_box.x;
                let real_y = y + bounding_box.y;
                let index = self.index(real_x, real_y);
                let index_end = index + bounding_box.width;
                let color_index = window.frame_buffer.index(x, y);
                let color_index_end = color_index + bounding_box.width;
                unsafe {
                    let color = window.frame_buffer.buffer.get_unchecked(color_index..color_index_end);
                    let buffer = self.buffer.get_unchecked_mut(index..index_end);
                    buffer.copy_from_slice(&color);
            }
        }
        */

        // (ouz-a):I like this version better, it's easy to read very flexible and could be simplfied with little more work
        //         not really sure about how to improve it's performance maybe we could use chunks and then copy slices but
        //         or we could
        self.copy_window_from_iterators(window);
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
            // For best performance, we map the real physical framebuffer memory
            // as write-combining using the PAT (on x86 only).
            // If PAT isn't available, fall back to disabling caching altogether.
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

pub fn buffer_indexer(
    buffer: &mut BorrowedSliceMappedPages<u32, Mutable>,
    buffer_width: usize,
    rect: Rect,
) -> impl Iterator<Item = &mut u32> {
    let width = buffer_width;
    let x = rect.x;
    let mut y = rect.y;
    let starter = ((width as isize * y) + x) as usize;
    let mut keeper = starter;
    let buffer = buffer
        .iter_mut()
        .enumerate()
        .filter(move |(size, _)| {
            if y >= rect.height as isize + rect.y as isize {
                return false;
            }
            if *size > starter && size % (keeper + rect.width) == 0 {
                y += 1;
                keeper = ((width as isize * y) + x) as usize;
            }
            if size >= &keeper {
                true
            } else {
                false
            }
        })
        .map(|(_, b)| b);
    buffer
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

    fn new_window(dimensions: &Rect, title: Option<String>) -> Arc<Mutex<Window>> {
        let mut manager = WINDOW_MANAGER.get().unwrap().lock();
        let len = manager.windows.len();

        manager.window_rendering_order.push(len);
        let window = Window::new(
            *dimensions,
            VirtualFrameBuffer::new(dimensions.width, dimensions.height).unwrap(),
            title,
        );
        let arc_window = Arc::new(Mutex::new(window));
        manager.windows.push(Arc::downgrade(&arc_window.clone()));
        arc_window
    }

    fn draw_windows(&mut self) {
        for order in self.window_rendering_order.iter() {
            self.v_framebuffer
                .copy_window_only(&mut self.windows[*order].upgrade().unwrap().lock());
        }
        for window in self.windows.iter() {
            window.upgrade().unwrap().lock().blank();
        }
    }

    fn draw_mouse(&mut self) {
        let bounding_box = self.mouse.on_screen_window(
            self.v_framebuffer.width as isize,
            self.v_framebuffer.height as isize,
        );
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

    fn update(&mut self) {
        self.v_framebuffer.blank();
        self.draw_windows();
        self.draw_mouse();
    }

    // TODO: Remove magic numbers
    fn update_mouse_position(&mut self, x: isize, y: isize) {
        let mut new_pos_x = self.mouse.x + x;
        let mut new_pos_y = self.mouse.y - y;

        // handle left
        if (new_pos_x + (self.mouse.width as isize / 2)) < 0 {
            new_pos_x = self.mouse.x;
        }

        if new_pos_x < 0 {
            new_pos_x = 0;
        }

        // handle right
        if new_pos_x > (self.v_framebuffer.width) as isize - 3 {
            new_pos_x = self.v_framebuffer.width as isize - 3;
        }

        // handle top
        if new_pos_y < 0 {
            new_pos_y = 0;
        }

        // handle bottom
        if new_pos_y > self.v_framebuffer.height as isize - 3 {
            new_pos_y = self.v_framebuffer.height as isize - 3;
        }

        self.mouse.x = new_pos_x;
        self.mouse.y = new_pos_y;
    }

    fn drag_windows(&mut self, x: isize, y: isize, mouse_event: &MouseEvent) {
        if mouse_event.buttons.left() {
            match self.mouse_holding {
                Holding::Background => todo!(),
                Holding::Nothing => {
                    // This costs nothing
                    let rendering_o = self.window_rendering_order.clone();
                    for &i in rendering_o.iter().rev() {
                        let window = &mut self.windows[i];
                        if window
                            .upgrade()
                            .unwrap()
                            .lock()
                            .return_dynamic_border_pos()
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
                    if new_pos_y < 0 {
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

// TODO: We need to two different coordinate systems, one for window oriented and one for screen oriented tasks
pub struct Window {
    rect: Rect,
    pub frame_buffer: VirtualFrameBuffer,
    resized: bool,
    title: Option<String>,
}

impl Window {
    fn new(rect: Rect, frame_buffer: VirtualFrameBuffer, title: Option<String>) -> Window {
        Window {
            rect,
            frame_buffer,
            resized: false,
            title,
        }
    }

    pub fn blank(&mut self) {
        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = 0x000000;
        }
    }

    /// Returns Window's border area width height with default position
    pub fn return_title_border(&self) -> Rect {
        let border = Rect::new(self.rect.width, TITLE_BAR_HEIGHT, 0, 0);
        border
    }

    pub fn return_dynamic_border_pos(&self) -> Rect {
        let mut rect = self.rect;
        rect.height = TITLE_BAR_HEIGHT;
        rect
    }

    // We don't want user to draw on top a border
    pub fn return_drawable_area(&self) -> Rect {
        let border = self.return_title_border();
        let x = 0;
        let y = border.height;
        let width = border.width;
        let height = self.rect.height - y;
        let drawable_area = Rect::new(width, height, x, y as isize);
        drawable_area
    }

    pub fn return_title_pos(&self, slice_len: &usize) -> RelativePos {
        let border = self.return_title_border();
        let pos = (border.width - (slice_len * CHARACTER_WIDTH)) / 2;
        let relative_pos = RelativePos::new(pos as u32, 0);
        relative_pos
    }

    pub fn draw_border(&mut self) {
        let border = self.return_title_border();
        let buffer = buffer_indexer(&mut self.frame_buffer.buffer, self.rect.width, border);
        for pixel in buffer {
            *pixel = DEFAULT_BORDER_COLOR;
        }
    }

    // I'm not exactly sure if using `unsafe` is right bet here
    // but since we are dealing with arrays/slices most of the time
    // we need to only prove they are within bounds once and this let's us safely call `unsafe`
    fn draw_unchecked(&mut self, relative_pos: &RelativePos, col: Color) {
        let x = relative_pos.x;
        let y = relative_pos.y;
        unsafe {
            let index = (self.frame_buffer.width * y as usize) + x as usize;
            let pixel = self.frame_buffer.buffer.get_unchecked_mut(index);
            *pixel = col;
        }
    }

    // TODO: add better(line,box..etc) drawing functions

    pub fn draw_rectangle(&mut self, col: Color) {
        // TODO: This should be somewhere else and it should be a function
        if self.resized {
            self.resize_framebuffer();
            self.resized = false;
        }
        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = col;
        }
        self.draw_border();
    }

    pub fn set_position(&mut self, x: isize, y: isize) {
        self.rect.x = x;
        self.rect.y = y;
    }

    fn resize_framebuffer(&mut self) {
        self.frame_buffer = VirtualFrameBuffer::new(self.rect.width, self.rect.height).unwrap();
    }

    /// Gives framebuffer iterator for the whole screen
    fn return_framebuffer_iterator(&mut self) -> impl Iterator<Item = &mut u32> {
        if self.bottom_side_out() || self.left_side_out() || self.right_side_out() {
            let mut bounding_box =
                self.on_screen_window(SCREEN_WIDTH as isize, SCREEN_HEIGHT as isize);
            bounding_box.x = 0;
            if self.left_side_out() {
                bounding_box.x = (self.rect.width - bounding_box.width) as isize;
            }
            bounding_box.y = 0;
            let buffer =
                buffer_indexer(&mut self.frame_buffer.buffer, self.rect.width, bounding_box);
            buffer
        } else {
            let rect = Rect::new(self.frame_buffer.width, self.frame_buffer.height, 0, 0);
            let buffer = buffer_indexer(&mut self.frame_buffer.buffer, self.rect.width, rect);
            buffer
        }
    }

    pub fn left_side_out(&self) -> bool {
        if self.rect.x < 0 {
            true
        } else {
            false
        }
    }
    pub fn right_side_out(&self) -> bool {
        if (self.rect.x + self.rect.width as isize) > SCREEN_WIDTH as isize {
            true
        } else {
            false
        }
    }

    pub fn bottom_side_out(&self) -> bool {
        if (self.rect.y + self.rect.height as isize) > SCREEN_HEIGHT as isize {
            true
        } else {
            false
        }
    }

    // TODO: This should be moved to somewhere else
    // and renamed
    pub fn on_screen_window(&self, screen_width: isize, screen_height: isize) -> Rect {
        let mut start_x = self.rect.x;
        let start_y = self.rect.y;
        let mut end_x = self.rect.width as isize;
        let mut end_y = self.rect.height as isize;
        if self.rect.x < 0 {
            start_x = 0;
            end_x = self.rect.x + self.rect.width as isize;
        } else if self.rect.x + self.rect.width as isize > screen_width as isize {
            start_x = self.rect.x;
            let gap = (self.rect.x + self.rect.width as isize) - screen_width as isize;
            end_x = self.rect.width as isize - gap;
        }
        if self.rect.y + self.rect.height as isize > screen_height {
            let gap = (self.rect.y + self.rect.height as isize) - screen_height;
            end_y = self.rect.height as isize - gap;
        }
        let f = Rect::new(end_x as usize, end_y as usize, start_x, start_y);
        f
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
                        window_manager.lock().update_mouse_position(x, y);
                        window_manager.lock().drag_windows(x, y, &mouse_event);
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
