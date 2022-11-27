use crate::*;

/// Controls amount of visible `Window` we see when we move a `Window` out of the screen
pub static WINDOW_VISIBLE_GAP: i32 = 20;

/// Controls amount of visible mouse we see when we move the mouse out of the screen
pub static MOUSE_VISIBLE_GAP: i32 = 3;
/// Height of the Window's title bar
pub static TITLE_BAR_HEIGHT: usize = 20;
pub struct Window {
    rect: Rect,
    pub frame_buffer: VirtualFrameBuffer,
    resized: bool,
    pub resizing: bool,
    title: Option<String>,
    title_border: Option<Rect>,
    title_pos: Option<RelativePos>,
    drawable_area: Option<Rect>,
    pub event: Queue<Event>,
    pub(crate) active: bool,
}

impl Window {
    pub(crate) fn new(
        rect: Rect,
        frame_buffer: VirtualFrameBuffer,
        title: Option<String>,
    ) -> Window {
        let events = Queue::with_capacity(100);
        Window {
            rect,
            frame_buffer,
            resized: false,
            resizing: false,
            title,
            title_border: None,
            title_pos: None,
            drawable_area: None,
            event: events,
            active: false,
        }
    }

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
        position: &mut RelativePos,
        string: &mut String,
        fg_color: Color,
        bg_color: Color,
    ) -> Result<(), &'static str> {
        for line in string.lines() {
            // Number of characters that can fit in a line
            let line_len = line.len() * CHARACTER_WIDTH;
            // If text fits to a single line
            if line_len < self.width() - CHARACTER_WIDTH {
                self.print_string_line(position, line, fg_color, bg_color)?;

                let mut window_rect = self.rect();
                window_rect.height = CHARACTER_HEIGHT - 1;
                let rest_of_the_line = window_rect.width - line_len;
                window_rect.width = rest_of_the_line;
                window_rect.y = position.y as isize;
                window_rect.x = line_len as isize;
                // We fill rest of the line with `bg_color` to clear the screen
                self.fill_rectangle(&mut window_rect, 0x1FF333);
                if position.y != self.height() as u32 {
                    position.y += CHARACTER_HEIGHT as u32;
                }
            } else {
                let max_text_width = self.width() / CHARACTER_WIDTH;
                let mut text_start = 0;
                while let Some(shorter_line) = line.get(text_start..) {
                    text_start += max_text_width;
                    if position.y >= 479 {
                        log::info!("position is {position:?}");
                        log::info!("shorter line is {shorter_line}");
                    }

                    self.print_string_line(position, shorter_line, fg_color, bg_color)?;

                    let mut window_rect = self.rect();
                    window_rect.height = CHARACTER_HEIGHT - 1;
                    let rest_of_the_line =
                        window_rect.width - (shorter_line.len() * CHARACTER_WIDTH);
                    window_rect.width = rest_of_the_line;
                    window_rect.y = position.y as isize;
                    window_rect.x = (shorter_line.len() * CHARACTER_WIDTH) as isize;
                    self.fill_rectangle(&mut window_rect, bg_color);

                    if position.y < self.height() as u32 {
                        position.y += CHARACTER_HEIGHT as u32;
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
        if !slice.is_empty() {
            let slice = slice.as_bytes();
            let start_x = position.x;
            let start_y = position.y;

            let mut x_index = 0;
            let mut row_controller = 0;
            let mut char_index = 0;
            let mut char_color_on_x_axis = x_index;

            let mut window_rect = self.rect();
            window_rect.set_position(start_x, start_y);
            // We want to get smmallest iterator possible for given `str` and `Rect`
            let min_width = core::cmp::min(self.rect.width(), slice.len() * CHARACTER_WIDTH);
            window_rect.width = min_width;

            let mut row_of_pixels = FramebufferRowChunks::get_exact_row(
                &mut self.frame_buffer,
                window_rect,
                start_y as usize,
            );

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
                        row_of_pixels = FramebufferRowChunks::get_exact_row(
                            &mut self.frame_buffer,
                            window_rect,
                            y as usize,
                        );
                        row_controller += 1;
                        char_index = 0;
                        x_index = 0;
                    }

                    if row_controller == CHARACTER_HEIGHT {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn display_window_title(
        &mut self,
        fg_color: Color,
        bg_color: Color,
    ) -> Result<(), &'static str> {
        if self.title.is_some() {
            let title = self
                .title
                .as_ref()
                .ok_or("Couldn't clone Window Tittle")?
                .clone();
            let slice = title.as_str();
            let title_pos = self.title_pos(&slice.len());
            self.print_string_line(&title_pos, slice, fg_color, bg_color)?;
        }
        Ok(())
    }

    pub fn width(&self) -> usize {
        self.rect.width
    }

    pub fn height(&self) -> usize {
        self.rect.height
    }

    pub fn fill_rectangle(&mut self, rect: &mut Rect, color: Color) {
        let width = self.width();
        if rect.x <= (self.rect.width() as isize - CHARACTER_WIDTH as isize)
            && rect.y <= (self.rect.height as isize - CHARACTER_HEIGHT as isize)
            && self.rect.width == self.frame_buffer.width
            && self.rect.height == self.frame_buffer.height
        {
            let row_chunks = FramebufferRowChunks::new(&mut self.frame_buffer, rect, width);
            for row in row_chunks {
                for pixel in row {
                    *pixel = color;
                }
            }
        }
    }

    pub fn screen_pos(&self) -> ScreenPos {
        let screen_pos = ScreenPos::new(self.rect.x as i32, self.rect.y as i32);
        screen_pos
    }

    pub fn set_screen_pos(&mut self, screen_position: &ScreenPos) {
        self.rect.x = screen_position.x as isize;
        self.rect.y = screen_position.y as isize;
    }

    /// Pushes an event into `self.event`
    pub fn push_event(&mut self, event: Event) -> Result<(), Event> {
        self.event.push(event)
    }

    /// Pops event from `self.event` and returns it
    pub fn pop_event(&self) -> Option<Event> {
        self.event.pop()
    }

    pub fn resize_window(&mut self, width: i32, height: i32) {
        // We clamp the values so resizing is not so jumpy and multiply them by CHARACTER_WIDTH
        // and CHARACTER_HEIGHT so the Window is almost always divisible by those values.
        let width = width.clamp(-1, 1) * CHARACTER_WIDTH as i32;
        let height = height.clamp(-1, 1) * CHARACTER_HEIGHT as i32;

        // We don't want any window to be smaller than 180 and bigger than screen itself
        let mut new_width = core::cmp::max(self.width() + width as usize, 180);
        new_width = core::cmp::min(SCREEN_WIDTH, new_width);
        let mut new_height = core::cmp::max(self.height() + height as usize, 180);
        new_height = core::cmp::min(SCREEN_HEIGHT, new_height);
        self.rect.width = new_width;
        self.rect.height = new_height;
        self.resized = true;
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

    pub fn draw_title_border(&mut self) {
        let mut border = self.title_border();
        let stride = self.frame_buffer.width;
        let rows = FramebufferRowChunks::new(&mut self.frame_buffer, &mut border, stride);
        for row in rows {
            for pixel in row {
                *pixel = DEFAULT_BORDER_COLOR;
            }
        }
    }

    /// Return's the window's `Rect`
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Clears the window screen back to it's default color
    pub fn clear(&mut self) {
        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = DEFAULT_WINDOW_COLOR;
        }
    }

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
        self.should_resize_framebuffer()?;

        for pixel in self.frame_buffer.buffer.iter_mut() {
            *pixel = color;
        }
        self.draw_title_border();
        Ok(())
    }

    /// Resizes framebuffer after to Window's width and height
    fn resize_framebuffer(&mut self) -> Result<(), &'static str> {
        self.frame_buffer = VirtualFrameBuffer::new(self.rect.width, self.rect.height)?;
        Ok(())
    }

    /// Returns visible part of self's `rect` with relative bounds applied:
    /// This is used for rendering a window when it's partially outside the screen,
    /// because we don't change window's framebuffer width and height when its outside screen coordinates
    /// we want to be able to render only parts of it:
    /// e.g if `self.rect: { width: 400, height: 400, x: -103, y: 0 }`,
    /// visible rect is `{ width: 297, height: 400, x: 0, y: 0 }`,
    /// this function will return `{ width: 297, height: 400, x: 103, y: 0 }`
    /// which will allow us to give the illusion of partially rendering the window.
    pub fn relative_visible_rect(&self) -> Rect {
        let mut visible_window = self.rect.visible_rect();
        visible_window.x = 0;
        if self.rect.left_side_out() {
            visible_window.x = (self.rect.width - visible_window.width) as isize;
        }
        visible_window.y = 0;
        visible_window
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
