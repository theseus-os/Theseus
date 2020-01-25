//! A `Window` object should be owned by an application. It can display a `Displayable` object in its framebuffer. See `applications/new_window` as a demo to use this library.
//!
//! This library will create a window with default title bar and border. It handles the commonly used interactions like moving
//! the window or close the window. Also, it is responsible to show title bar differently when window is active. 
//!
//! A window can render itself to the screen via a window manager. The window manager will compute the bounding box of the updated part and composites it with other existing windows according to their order.
//!
//! The library
//! frees applications from handling the complicated interaction with window manager, however, advanced users could learn from
//! this library about how to use window manager APIs directly.
//!

#![no_std]
extern crate alloc;
extern crate mpmc;
extern crate event_types;
extern crate spin;
#[macro_use]
extern crate log;
extern crate framebuffer;
extern crate framebuffer_drawer;
extern crate mouse;
extern crate window_inner;
extern crate window_manager;
extern crate shapes;
extern crate color;

use alloc::sync::Arc;
use mpmc::Queue;
use event_types::{Event, MousePositionEvent};
use framebuffer::Framebuffer;
use color::{Color};
use shapes::{Coord, Rectangle};
use spin::Mutex;
use window_inner::{WindowInner, WindowMovingStatus, DEFAULT_BORDER_SIZE, DEFAULT_TITLE_BAR_HEIGHT};
use window_manager::{WINDOW_MANAGER};


// border radius, in number of pixels
const WINDOW_RADIUS: usize = 5;
// border and title bar color when window is inactive
const WINDOW_BORDER_COLOR_INACTIVE: Color = Color::new(0x00333333);
// border and title bar color when window is active, the top part color
const WINDOW_BORDER_COLOR_ACTIVE_TOP: Color = Color::new(0x00BBBBBB);
// border and title bar color when window is active, the bottom part color
const WINDOW_BORDER_COLOR_ACTIVE_BOTTOM: Color = Color::new(0x00666666);
// window button color: red
const WINDOW_BUTTON_COLOR_CLOSE: Color = Color::new(0x00E74C3C);
// window button color: green
const WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE: Color = Color::new(0x00239B56);
// window button color: purple
const WINDOW_BUTTON_COLOR_HIDE: Color = Color::new(0x007D3C98);
// window button margin from left, in number of pixels
const WINDOW_BUTTON_BIAS_X: usize = 12;
// the interval between buttons, in number of pixels
const WINDOW_BUTTON_BETWEEN: usize = 15;
// the button size, in number of pixels
const WINDOW_BUTTON_SIZE: usize = 6;

// The buttons shown in title bar
enum TopButton {
    // Button to close the window
    Close,
    // Button to minimize/maximize the window (depends on the current state)
    MinimizeMaximize,
    // Button to hide the window
    Hide,
}

impl From<usize> for TopButton {
    fn from(item: usize) -> Self {
        match item {
            0 => TopButton::Close,
            1 => TopButton::MinimizeMaximize,
            2 => TopButton::Hide,
            _ => TopButton::Close,
        }
    }
}

/// Abstraction of a window which owns a framebuffer and the window's handler. It provides title bar which helps user moving, close, maximize or minimize window
pub struct Window {
    /// this object contains states and methods required by the window manager
    pub inner: Arc<Mutex<WindowInner>>,
    /// application could get events from this consumer
    pub consumer: Queue<Event>,
    /// event output used by window manager, private variable
    producer: Queue<Event>,
    /// last mouse position event, used to judge click and press-moving event
    last_mouse_position_event: MousePositionEvent,
    /// record last result of whether this window is active, to reduce redraw overhead
    last_is_active: bool,
}

impl Window {
    /// Creates a new Window at `coordinate` relative to the top-left of the screen and adds it to the window manager.
    /// `width`, `height` represent the size of the window in number of pixels.
    /// `background` is the background color of the window. Currently the window is based on alpha framebuffer, so the pixel value of the background has an alpha channel and RGB bytes.
    pub fn new(
        coordinate: Coord,
        width: usize,
        height: usize,
        background: Color,
    ) -> Result<Window, &'static str> {
        let framebuffer = Framebuffer::new(width, height, None)?;
        let (width, height) = framebuffer.get_size();
        if width <= 2 * DEFAULT_TITLE_BAR_HEIGHT || height <= DEFAULT_TITLE_BAR_HEIGHT + DEFAULT_BORDER_SIZE {
            return Err("window dimensions must be large enough for the title bar and borders to be drawn");
        }

        // create event queue for components
        let consumer = Queue::with_capacity(100);
        let producer = consumer.clone();

        let mut window = Window {
            inner: Arc::new(Mutex::new(WindowInner::new(coordinate, framebuffer, background)?)),
            consumer: consumer,
            producer: producer,
            last_mouse_position_event: MousePositionEvent::default(),
            last_is_active: true, // new window is now set as the active window by default 
        };

        // draw window with active border
        window.draw_border(true);
        // draw three buttons
        {
            let mut inner = window.inner.lock();
            window.show_button(TopButton::Close, 1, &mut inner);
            window.show_button(TopButton::MinimizeMaximize, 1, &mut inner);
            window.show_button(TopButton::Hide, 1, &mut inner);
        }

        let area = Rectangle {
            top_left: coordinate,
            bottom_right: coordinate + (width as isize, height as isize)
        };

        let mut wm = window_manager::WINDOW_MANAGER.try().ok_or("The window manager is not initialized")?.lock();
        let first_active = wm.set_active(&window.inner, false)?; 
        let bounding_box = if first_active {
            None
        } else {
            Some(area)
        };
        wm.refresh_bottom_windows(bounding_box, true)?;
        Ok(window)
    }


    /// Handles the event sent to the window's inner event queue by window manager.
    /// 
    /// Currently, if an event is not handled here, it is pushed 
    pub fn handle_event(&mut self) -> Result<(), &'static str> {
        let mut call_later_do_refresh_floating_border = false;
        let mut call_later_do_move_active_window = false;
        let mut need_to_set_active = false;
        let mut need_refresh_three_button = false;

        let wm_mut = window_manager::WINDOW_MANAGER.try().ok_or("The window manager is not initialized")?;
        
        let is_active = {
            let wm = wm_mut.lock();
            wm.is_active(&self.inner)
        };
        if is_active != self.last_is_active {
            self.draw_border(is_active);
            self.last_is_active = is_active;
            let mut inner = self.inner.lock();
            self.show_button(TopButton::Close, 1, &mut inner);
            self.show_button(TopButton::MinimizeMaximize, 1, &mut inner);
            self.show_button(TopButton::Hide, 1, &mut inner);
        }

        loop {
            let mut inner = self.inner.lock();
            let (width, height) = inner.get_size();
            let event = match inner.consumer.pop() {
                Some(ev) => ev,
                None => break,
            };

            match event {
                Event::MousePositionEvent(ref mouse_event) => {
                    match inner.moving {
                        WindowMovingStatus::Moving(_) => {
                            // only wait for left button up to exit this mode
                            if !mouse_event.left_button_hold {
                                self.last_mouse_position_event = mouse_event.clone();
                                call_later_do_move_active_window = true;
                            }
                            call_later_do_refresh_floating_border = true;
                        },
                        WindowMovingStatus::Stationary => {
                            if (mouse_event.coordinate.y as usize) < inner.title_bar_height
                                && (mouse_event.coordinate.x as usize) < width
                            {
                                // the region of title bar
                                let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
                                let mut is_three_button = false;
                                for i in 0..3 {
                                    let dcoordinate = Coord::new(
                                        mouse_event.coordinate.x
                                            - WINDOW_BUTTON_BIAS_X as isize
                                            - (i as isize) * WINDOW_BUTTON_BETWEEN as isize,
                                        mouse_event.coordinate.y - inner.title_bar_height as isize / 2,
                                    );
                                    if dcoordinate.x * dcoordinate.x + dcoordinate.y * dcoordinate.y
                                        <= r2 as isize
                                    {
                                        is_three_button = true;
                                        if mouse_event.left_button_hold {
                                            self.show_button(TopButton::from(i), 2, &mut inner);
                                            need_refresh_three_button = true;
                                        } else {
                                            self.show_button(TopButton::from(i), 0, &mut inner);
                                            need_refresh_three_button = true;
                                            if self.last_mouse_position_event.left_button_hold {
                                                // Kevin: disabling the close button until it actually works
                                                /*
                                                // click event
                                                if i == 0 {
                                                    debug!("close window");
                                                    return Err("user close window");
                                                    // window will not close until app drop self
                                                }
                                                */
                                            }
                                        }
                                    } else {
                                        self.show_button(TopButton::from(i), 1, &mut inner);
                                        need_refresh_three_button = true;
                                    }
                                }
                                // check if user clicked and held the title bar, which means user wanted to move the window
                                if !is_three_button
                                    && !self.last_mouse_position_event.left_button_hold
                                    && mouse_event.left_button_hold
                                {
                                    inner.moving = WindowMovingStatus::Moving(mouse_event.gcoordinate);
                                    call_later_do_refresh_floating_border = true;
                                }
                            } else {
                                // The mouse event occurred within the actual window content, not in the title bar.
                                // Thus, we push it into the "outer" window queue so applications can handle it.
                                self.producer.push(Event::MousePositionEvent(mouse_event.clone()))
                                    .map_err(|_e| "Failed to push the mouse event from inner to outer queue")?;
                            }
                            if (mouse_event.coordinate.y as usize) < height
                                && (mouse_event.coordinate.x as usize) < width
                                && !self.last_mouse_position_event.left_button_hold
                                && mouse_event.left_button_hold
                            {
                                need_to_set_active = true;
                            }
                            self.last_mouse_position_event = mouse_event.clone();
                        }
                    }
                }
                unhandled => {
                    // push unhandled events from the window's inner queue onto the window's "outer" queue
                    self.producer.push(unhandled)
                        .map_err(|_e| "Failed to push unhandled event from inner to outer queue")?;
                }
            }
            // event.mark_completed();
        }

        let mut wm = wm_mut.lock();
        if need_to_set_active {
            wm.set_active(&self.inner, true)?;
        }

        if need_refresh_three_button {
            let area = self.get_button_area();
            wm.refresh_active_window(Some(area))?;
            wm.refresh_mouse()?;
        }

        if call_later_do_refresh_floating_border {
            wm.move_floating_border()?;
        }

        if call_later_do_move_active_window {
            wm.move_active_window()?;
            self.inner.lock().moving = WindowMovingStatus::Stationary;
        }

        Ok(())
    }

    /// Render the part of the window in `bounding_box` to the screen. Refresh the whole window if `bounding_box` is `None`. The method should be invoked after updating.
    pub fn render(&mut self, bounding_box: Option<Rectangle>) -> Result<(), &'static str> {
        let coordinate = {
            let window = self.inner.lock();
            window.get_position()
        };

        let mut wm = WINDOW_MANAGER
            .try()
            .ok_or("The static window manager was not yet initialized")?
            .lock();

        let absolute_box = match bounding_box {
            Some(bounding_box) => Some(bounding_box + coordinate),
            None => None,
        };
        wm.refresh_windows(absolute_box)
    }

    /// Draw the border of this window, with argument of whether this window is active now
    fn draw_border(&mut self, active: bool) {
        let mut inner = self.inner.lock();
        let border_size = inner.border_size;
        let title_bar_height = inner.title_bar_height;

        // first draw left, bottom, right border
        let border_color = if active {
            WINDOW_BORDER_COLOR_ACTIVE_BOTTOM
        } else {
            WINDOW_BORDER_COLOR_INACTIVE
        };
        let (width, height) = inner.get_size();

        framebuffer_drawer::draw_rectangle(
            &mut inner.framebuffer,
            Coord::new(0, title_bar_height as isize),
            border_size,
            height - title_bar_height,
            border_color.into(),
        );

        framebuffer_drawer::draw_rectangle(
            &mut inner.framebuffer,
            Coord::new(0, (height - border_size) as isize),
            width,
            border_size,
            border_color.into(),
        );
        framebuffer_drawer::draw_rectangle(
            &mut inner.framebuffer,
            Coord::new(
                (width - border_size) as isize,
                title_bar_height as isize,
            ),
            border_size,
            height - title_bar_height,
            border_color.into(),
        );

        // then draw the title bar
        if active {
            for i in 0..title_bar_height {
                framebuffer_drawer::draw_rectangle(
                    &mut inner.framebuffer,
                    Coord::new(0, i as isize),
                    width,
                    1,
                    framebuffer::Pixel::weight_blend(
                        WINDOW_BORDER_COLOR_ACTIVE_BOTTOM.into(),
                        WINDOW_BORDER_COLOR_ACTIVE_TOP.into(),
                        (i as f32) / (title_bar_height as f32)
                    )
                ); 
            }
        } else {
            framebuffer_drawer::draw_rectangle(
                &mut inner.framebuffer,
                Coord::new(0, 0),
                width,
                title_bar_height,
                border_color.into(),
            );
        }

        // draw radius finally
        let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
        let trans_pixel = color::TRANSPARENT.into();
  
        for i in 0..WINDOW_RADIUS {
            for j in 0..WINDOW_RADIUS {
                let dx1 = WINDOW_RADIUS - i;
                let dy1 = WINDOW_RADIUS - j;
                if dx1 * dx1 + dy1 * dy1 > r2 {
                    // draw this to transparent
                    inner.framebuffer
                        .overwrite_pixel(Coord::new(i as isize, j as isize), trans_pixel);
                    inner.framebuffer.overwrite_pixel(
                        Coord::new((width - i - 1) as isize, j as isize), trans_pixel);
                }
            }
        }
    }

    /// show three button with status. state = 0,1,2 for three different color
    fn show_button(&self, button: TopButton, state: usize, inner: &mut WindowInner) {
        let y = inner.title_bar_height / 2;
        let x = WINDOW_BUTTON_BIAS_X
            + WINDOW_BUTTON_BETWEEN
                * match button {
                    TopButton::Close => 0,
                    TopButton::MinimizeMaximize => 1,
                    TopButton::Hide => 2,
                };
        let color = match button {
            TopButton::Close => WINDOW_BUTTON_COLOR_CLOSE,
            TopButton::MinimizeMaximize => WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE,
            TopButton::Hide => WINDOW_BUTTON_COLOR_HIDE,
        };
        framebuffer_drawer::draw_circle(
            &mut inner.framebuffer,
            Coord::new(x as isize, y as isize),
            WINDOW_BUTTON_SIZE,
            framebuffer::Pixel::weight_blend(
                color::BLACK.into(), 
                color.into(),
                0.2f32 * (state as f32),
            ),
        );
    }

    /// Gets the rectangle occupied by the three buttons
    fn get_button_area(&self) -> Rectangle {
        let inner = self.inner.lock();
        let width = inner.get_size().0;
        Rectangle {
            top_left: Coord::new(0, 0),
            bottom_right: Coord::new(width as isize, inner.title_bar_height as isize)
        }
    }
}


impl Drop for Window{
    fn drop(&mut self){
        if let Some(wm) = WINDOW_MANAGER.try() {
            if let Err(err) = wm.lock().delete_window(&self.inner) {
                error!("Failed to delete_window upon drop: {:?}", err);
            }
        } else {
            error!("BUG: Could not delete_window upon drop because the window manager was not initialized");
        }
    }
}