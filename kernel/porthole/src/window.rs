use crate::*;

use ringbuffer::*;

/// Controls amount of visible `Window` we see when we move a `Window` out of the screen
pub static WINDOW_VISIBLE_GAP: i32 = 20;

/// Controls amount of visible mouse we see when we move the mouse out of the screen
pub static MOUSE_VISIBLE_GAP: i32 = 3;
/// Height of the Window's title bar
pub static TITLE_BAR_HEIGHT: usize = 20;

/// Size of the side border gap
pub static SIDE_BORDER_GAP: usize = 4;
/// Size of the bottom border gap
pub static BOTTOM_BORDER_GAP: usize = 1;

/// Represents a window that can be drawn to and interacted with
pub struct Window {
    /// The rectangle defining the position and size of the window.
    rect: Rect,

    /// The virtual frame buffer for the window, which is used to draw its contents.
    pub frame_buffer: VirtualFramebuffer,

    /// Whether or not the window has been resized
    resized: bool,

    /// Whether or not the window is currently being resized by the user.
    pub resizing: bool,

    /// The title of the window, if it has one.
    title: Option<String>,

    /// The rectangle defining the position and size of the title bar, if the window has a title.
    title_border: Option<Rect>,

    /// The relative position of the title within the title bar, if the window has a title.
    title_pos: Option<RelativePos>,

    /// The area within the window where drawing can occur, excluding the title bar and any borders.
    drawable_area: Option<Rect>,

    /// A queue of events for the window.
    pub event_queue: ConstGenericRingBuffer<Event, 128>,

    /// Whether or not the window should receive events.
    pub receive_events: bool,

    /// Whether or not the window is currently active (focused).
    pub(crate) active: bool,
}

impl Window {
    pub(crate) fn new(
        rect: Rect,
        frame_buffer: VirtualFramebuffer,
        title: Option<String>,
        receive_events: bool,
    ) -> Window {
        Window {
            rect,
            frame_buffer,
            resized: false,
            resizing: false,
            title,
            title_border: None,
            title_pos: None,
            drawable_area: None,
            event_queue: ConstGenericRingBuffer::new(),
            receive_events,
            active: false,
        }
    }

    /// Creates a new `Window`, with given dimensions and an optional title.
    pub fn new_window(
        rect: &Rect,
        title: Option<String>,
        receive_events: bool,
    ) -> Result<Arc<Mutex<Window>>, &'static str> {
        let mut window_manager = WINDOW_MANAGER
            .get()
            .ok_or("Failed to get WindowManager while creating a window")?
            .lock();
        let len = window_manager.windows.len();

        window_manager.window_rendering_order.push(len);
        let window = Window::new(
            *rect,
            VirtualFramebuffer::new(rect.width, rect.height)?,
            title,
            receive_events,
        );
        let arc_window = Arc::new(Mutex::new(window));
        arc_window.lock().active = true;
        let returned_window = arc_window.clone();
        window_manager.windows.push(arc_window);
        Ok(returned_window)
    }

    /// Returns whether window is active or not
    pub fn active(&self) -> bool {
        self.active
    }

    /// Prints a string onto the window
    ///
    /// * `position` - This indicates where line of text will be.
    /// * `string` - Text we are printing
    /// * `fg_color` - Foreground color of the text
    /// * `bg_color` - Background color of the text
    pub fn print_string(
        &mut self,
        position: RelativePos,
        string: &String,
        fg_color: Color,
        bg_color: Color,
    ) -> Result<(), &'static str> {
        let mut position = position;
        for line in string.lines() {
            // Number of characters that can fit in a line
            let line_len = line.len() * CHARACTER_WIDTH;
            // If text fits to a single line
            if line_len < self.drawable_area().width() - CHARACTER_WIDTH {
                self.print_string_line(&position, line, fg_color, bg_color)?;
                self.fill_rest_of_line_blank(line.len(), bg_color, position.y as isize);
                if position.y <= self.drawable_area().height as u32 {
                    position.y += CHARACTER_HEIGHT as u32;
                }
            } else {
                let max_text_width = self.drawable_area().width() / CHARACTER_WIDTH;
                let mut text_start = 0;
                while let Some(shorter_line) = line.get(text_start..) {
                    let text_end = core::cmp::min(shorter_line.len(), max_text_width);
                    let shorter_line = shorter_line.get(..text_end).unwrap();
                    text_start += max_text_width;
                    self.print_string_line(&position, shorter_line, fg_color, bg_color)?;
                    self.fill_rest_of_line_blank(shorter_line.len(), bg_color, position.y as isize);
                    if position.y <= self.drawable_area().height as u32 {
                        position.y += CHARACTER_HEIGHT as u32;
                    }
                }
            }
        }
        Ok(())
    }

    /// Fills the remaining width of the current line with a blank space of the specified color
    ///
    /// * `text_len` - The length of the text that has been printed on the line so far
    /// * `color` - The background color to fill the remaining space with
    /// * `y` - The vertical position of the line
    fn fill_rest_of_line_blank(&mut self, text_len: usize, color: Color, y: isize) {
        // Calculate the width of the text that has been printed so far
        let text_width = text_len * CHARACTER_WIDTH;

        // Calculate the area of the current line that has not yet been printed on
        let mut drawable_area = self.drawable_area();
        drawable_area.y = y;
        drawable_area.height = 900;
        drawable_area.x += text_width as isize;
        drawable_area.width -= text_width;

        // Fill the remaining space on the current line with the specified color
        self.fill_rectangle(drawable_area, color);
    }

    /// Prints a string on a new line at the specified absolute (x, y) coordinates.
    fn print_string_line_abs(
        &mut self,
        x: u32,
        y: u32,
        slice: &str,
        fg_color: Color,
        bg_color: Color,
    ) -> Result<(), &'static str> {
        if !slice.is_empty() {
            let slice = slice.as_bytes();
            let start_x = x;
            let start_y = y;

            let mut x_index = 0;
            let mut row_controller = 0;
            let mut char_index = 0;
            let mut char_color_on_x_axis = x_index;

            let mut window_rect = self.drawable_area();
            window_rect.set_position(start_x, start_y);
            // We want to get smmallest iterator possible for given `str` and `Rect`
            let min_width = core::cmp::min(
                self.drawable_area().width() - 1,
                slice.len() * CHARACTER_WIDTH,
            );
            window_rect.width = min_width;

            let mut row_of_pixels = FramebufferRowIter::new(&mut self.frame_buffer,window_rect).next().unwrap().iter_mut();

            loop {
                let y = start_y + row_controller as u32;
                if x_index % CHARACTER_WIDTH == 0 {
                    char_color_on_x_axis = 0;
                }
                let color = if char_color_on_x_axis >= 1 {
                    let index = char_color_on_x_axis - 1;
                    let char_font = *FONT_BASIC
                        .get(*slice.get(char_index).unwrap_or(&32) as usize)
                        .ok_or("Couldn't find corresponding font for the char")?
                        .get(row_controller)
                        .ok_or("Couldn't find corresponding pixel for the char")?;
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

                // Altough bit ugly, this works quite well with our current way of rendering fonts
                if let Some(pixel) = row_of_pixels.next() {
                    *pixel = color;
                }

                x_index += 1;
                if x_index == CHARACTER_WIDTH || x_index % CHARACTER_WIDTH == 0 {
                    if slice.len() >= 1 && char_index < slice.len() - 1 {
                        char_index += 1;
                    }

                    if x_index >= CHARACTER_WIDTH * slice.len()
                        && x_index % (CHARACTER_WIDTH * slice.len()) == 0
                    {
                        window_rect.y = y as isize;
                        row_of_pixels = FramebufferRowIter::new(&mut self.frame_buffer,window_rect).next().unwrap().iter_mut();
                        row_controller += 1;
                        char_index = 0;
                        x_index = 1;
                    }

                    if row_controller == CHARACTER_HEIGHT {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Prints a line of string to the onto the window
    ///  
    /// * `position` - This indicates where line of text will be.
    /// * `slice` - Text we are printing
    /// * `fg_color` - Foreground color of the text
    /// * `bg_color` - Background color of the text
    pub fn print_string_line(
        &mut self,
        position: &RelativePos,
        slice: &str,
        fg_color: Color,
        bg_color: Color,
    ) -> Result<(), &'static str> {
        let (x, y) = self.to_absolute_pos(position);
        self.print_string_line_abs(x, y, slice, fg_color, bg_color)
    }

    /// Displays the window title on the screen.
    ///
    /// The title will be printed in the color specified by `fg_color`, with a background color of `bg_color`.
    pub fn display_window_title(
        &mut self,
        fg_color: Color,
        bg_color: Color,
    ) -> Result<(), &'static str> {
        if let Some(title) = self.title.take() {
            let slice = title.as_str();
            let title_pos = self.title_pos(&slice.len());
            self.print_string_line_abs(title_pos.x, title_pos.y, slice, fg_color, bg_color)?;
            self.title = Some(title);
        }
        Ok(())
    }

    /// Return's width of the window
    fn width(&self) -> usize {
        self.rect.width
    }

    /// Return's height of the window
    fn height(&self) -> usize {
        self.rect.height
    }

    /// Fill a rectangle on the window with a given color.
    ///
    /// # Arguments
    ///
    /// * `rect` - The rect we will fill inside the window with
    /// * `color` - The `Color` to fill the rectangle inside the window with.
    pub fn fill_rectangle(&mut self, rect: Rect, color: Color) {
        let fitted_rect = self.fit_rect_to_window(rect);
        self.fill_rect_abs(fitted_rect, color);
    }

    /// Fill a rectangle with absolute position on to the window.
    fn fill_rect_abs(&mut self, rect: Rect, color: Color) {
        if rect.x <= (self.rect.width() as isize as isize)
            && rect.y <= (self.rect.height as isize as isize)
            && self.rect.width == self.frame_buffer.width()
            && self.rect.height == self.frame_buffer.height()
        {
            let row_chunks = FramebufferRowIter::new(&mut self.frame_buffer, rect);
            row_chunks.for_each(|row| row.iter_mut().for_each(|pixel| *pixel = color));
        }
    }

    /// Returns ScreenPos of the window
    pub fn screen_pos(&self) -> ScreenPos {
        let screen_pos = ScreenPos::new(self.rect.x as i32, self.rect.y as i32);
        screen_pos
    }

    /// Set's window position to screen_position
    pub fn set_screen_pos(&mut self, screen_position: &ScreenPos) {
        self.rect.x = screen_position.x as isize;
        self.rect.y = screen_position.y as isize;
    }

    /// Pushes an event into `self.event`
    pub fn push_event(&mut self, event: Event) {
        self.event_queue.enqueue(event);
    }

    /// Pops event from `self.event` and returns it
    pub fn pop_event(&mut self) -> Option<Event> {
        self.event_queue.dequeue()
    }

    /// Resizes the window to the specified dimensions, clamping the values so that resizing is not too extreme
    /// and ensuring that the resulting width and height are divisible by `CHARACTER_WIDTH` and `CHARACTER_HEIGHT`.
    ///
    /// The minimum window size is 180 and the maximum is the screen size.
    pub fn resize_window(&mut self, width: i32, height: i32) {
        // Clamp the values so resizing is not too extreme, and multiply by CHARACTER_WIDTH and CHARACTER_HEIGHT
        // so the resulting width and height are almost always divisible by those values.
        let width = width.clamp(-1, 1) * CHARACTER_WIDTH as i32;
        let height = height.clamp(-1, 1) * CHARACTER_HEIGHT as i32;

        // Ensure the resulting width and height are within the minimum and maximum limits
        let new_width =
            (self.width() as i32 + width as i32).clamp(180, SCREEN_WIDTH as i32) as usize;
        let new_height =
            (self.height() as i32 + height as i32).clamp(180, SCREEN_HEIGHT as i32) as usize;

        // Update the window's dimensions and set the "resized" flag to true
        self.rect.width = new_width;
        self.rect.height = new_height;
        self.resized = true;
    }

    /// Set's drawable area to None
    pub fn reset_drawable_area(&mut self) {
        self.drawable_area = None;
    }

    /// Set's title_border and title_pos to None
    pub fn reset_title_pos_and_border(&mut self) {
        self.title_border = None;
        self.title_pos = None;
    }

    /// Returns Window's border area as a Rect
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

    /// Transforms a given `relative_pos` to an absolute position within the window
    ///
    /// Takes in a `RelativePos` and returns a tuple containing the transformed
    /// x and y coordinates. The transformed position ensures that the user cannot modify the window
    /// borders by clamping the x and y coordinates so it falls between vertical and horizontal bounds
    fn to_absolute_pos(&self, relative_pos: &RelativePos) -> (u32, u32) {
        let mut x = relative_pos.x;
        x = x.clamp(
            SIDE_BORDER_GAP as u32,
            (self.width() - SIDE_BORDER_GAP) as u32,
        );
        let mut y = relative_pos.y;
        y = y.clamp(
            TITLE_BAR_HEIGHT as u32,
            (self.height() - BOTTOM_BORDER_GAP) as u32,
        );
        (x, y)
    }

    /// Returns a new instance of the given `rect` that is bounded to fit within this Window.
    fn fit_rect_to_window(&mut self, rect: Rect) -> Rect {
        let (abs_x, abs_y) = self.to_absolute_pos(&rect.to_relative_pos());
        let new_x = abs_x as isize;
        let new_y = abs_y as isize;

        // If the rectangle extends beyond the right edge of the window,
        // reduce the rect's width to fit within the drawable area.
        let new_width = if rect.x_plus_width() >= self.drawable_area().width() as isize {
            let drawable_area_width = self.drawable_area().width();
            drawable_area_width - abs_x as usize
        } else {
            rect.width()
        };

        let new_height = if rect.y_plus_height() >= self.drawable_area().width() as isize{
            let drawable_area_height = self.drawable_area().height();
            drawable_area_height - abs_y as usize
        } else {
            rect.height()
        };

        Rect::new(new_width, new_height, new_x, new_y)
    }

    /// Returns a `Rect` representing the drawable area within the window.
    ///
    /// The drawable area is the region within the window where things can be drawn.
    /// This computes and returns the size and position of the drawable area based
    /// on the size of the window and its borders.
    pub fn drawable_area(&mut self) -> Rect {
        let title_bar_border = self.title_border();

        // Compute the size and position of the drawable area.
        let drawable_area = self.drawable_area.get_or_insert({
            let x = SIDE_BORDER_GAP;
            let y = title_bar_border.height;
            let width = title_bar_border.width - SIDE_BORDER_GAP;
            let height = (self.rect.height - y) - BOTTOM_BORDER_GAP;
            let drawable_area = Rect::new(width, height, x as isize, y as isize);
            drawable_area
        });

        *drawable_area
    }

    /// From given title length returns center position of the title border
    pub fn title_pos(&mut self, title_length: &usize) -> RelativePos {
        let border = self.title_border();
        let relative_pos = self.title_pos.get_or_insert({
            let pos = (border.width - (title_length * CHARACTER_WIDTH)) / 2;
            let relative_pos = RelativePos::new(pos as u32, 0);
            relative_pos
        });
        *relative_pos
    }

    /// Draws the borders of the Window using the default border color.
    pub fn draw_borders(&mut self) {
        let border = self.title_border();
        self.fill_rect_abs(border, DEFAULT_BORDER_COLOR);
        let rect = self.rect();
        let drawable_area = self.drawable_area();
        // Left border
        self.fill_rect_abs(
            Rect::new(1, drawable_area.height, 0, drawable_area.y),
            DEFAULT_BORDER_COLOR,
        );
        // Right border
        self.fill_rect_abs(
            Rect::new(
                1,
                drawable_area.height,
                (rect.width() - 1) as isize,
                drawable_area.y,
            ),
            DEFAULT_BORDER_COLOR,
        );
        // Bottom border
        self.fill_rect_abs(
            Rect::new(rect.width(), 1, 0, (rect.height() - 1) as isize),
            DEFAULT_BORDER_COLOR,
        );
    }

    /// Return's the window's `Rect`
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Clears the window screen back to it's default color
    pub fn clear(&mut self) {
        for pixel in self.frame_buffer.buffer_mut().iter_mut() {
            *pixel = DEFAULT_WINDOW_COLOR;
        }
    }

    /// Return's true if window is resized
    pub fn resized(&self) -> bool {
        self.resized
    }

    /// If the window is resized, resizes window's framebuffer
    pub fn should_resize_framebuffer(&mut self) -> Result<(), &'static str> {
        if self.resized() {
            self.resize_framebuffer()?;
            self.resized = false;
        }
        Ok(())
    }

    /// Fill the window with specified color
    pub fn fill(&mut self, color: Color) -> Result<(), &'static str> {
        // We do this check here because we don't want to resize window's framebuffer
        // as fast as possible, allocating and de-allocating that big of a memory sometimes causes system freezes/crashes
        // so we do the resize check here. Plus to fill the window we need updated version of framebuffer's width and height.
        self.should_resize_framebuffer()?;

        let window_background_area = self.rect();
        self.fill_rect_abs(window_background_area, color);

        self.draw_borders();
        Ok(())
    }

    /// Resizes framebuffer after to Window's width and height
    fn resize_framebuffer(&mut self) -> Result<(), &'static str> {
        self.frame_buffer = VirtualFramebuffer::new(self.rect.width, self.rect.height)?;
        Ok(())
    }

    /// Returns the visible part of this window's `rect`, relative to its bounds.
    /// This is used when we render a window when it is partially outside the screen.
    /// When a window is partially outside the screen, we do not change the framebuffer's
    /// width and height, so we need to be able to render only parts of it.
    ///
    /// For example, if `self.rect` is `{ width: 400, height: 400, x: -103, y: 0 }`,
    /// the visible rect is `{ width: 297, height: 400, x: 0, y: 0 }`.
    ///
    /// This function returns `{ width: 297, height: 400, x: 103, y: 0 }`, which allows us
    /// to give the illusion of partially rendering the window.
    pub fn relative_visible_rect(&self) -> Rect {
        // Get the visible part of the window's rect.
        let mut visible_rect = self.rect.visible_rect();

        // Set the x-coordinate to 0, since we want to render only the visible part.
        visible_rect.x = 0;

        // If the left side of the window is out of bounds, we need to adjust the x-coordinate.
        if self.rect.left_side_out() {
            // The visible part of the window's rect is at most `self.rect.width` wide.
            // Subtract the width of the visible part from the width of the window to get the
            // distance between the left edge of the visible part and the left edge of the window.
            let distance_from_left_edge = self.rect.width - visible_rect.width;
            visible_rect.x = distance_from_left_edge as isize;
        }

        // Set the y-coordinate to 0, since we want to render only the visible part.
        visible_rect.y = 0;

        // Return the visible part of the window's rect, relative to its bounds.
        visible_rect
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
    pub text: String,
    pub fg_color: Color,
    pub bg_color: Color,
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

    pub fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
    }
}
