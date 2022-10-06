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
extern crate owning_ref;
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
use owning_ref::{MutexGuardRef, MutexGuardRefMut};
use framebuffer::{Framebuffer, AlphaPixel};
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


/// This struct is the application-facing representation of a window.
/// 
pub struct Window {
    /// The system-facing inner representation of this window.
    /// The window manager interacts with this object directly;
    /// thus, applications should not be able to access this directly. 
    /// 
    /// This is wrapped in an `Arc` such that the window manager can hold `Weak` references to it.
    inner: Arc<Mutex<WindowInner>>,
    /// The event queue
    event_consumer: Queue<Event>,
    /// last mouse position event, used to judge click and press-moving event
    /// TODO FIXME (kevinaboos): why is mouse-specific stuff here? 
    last_mouse_position_event: MousePositionEvent,
    /// record last result of whether this window is active, to reduce redraw overhead
    last_is_active: bool,
}

impl Window {
    /// Creates a new window to be displayed on screen. 
    /// 
    /// The given `framebuffer` will be filled with the `initial_background` color.
    /// 
    /// The newly-created `Window` will be set as the "active" window that has current focus. 
    /// 
    /// # Arguments: 
    /// * `coordinate`: the position of the window relative to the top-left corner of the screen.
    /// * `width`, `height`: the dimensions of the window in pixels.
    /// * `initial_background`: the default color of the window.
    pub fn new(
        coordinate: Coord,
        width: usize,
        height: usize,
        initial_background: Color,
    ) -> Result<Window, &'static str> {
        let wm_ref = window_manager::WINDOW_MANAGER.get().ok_or("The window manager is not initialized")?;

        // Create a new virtual framebuffer to hold this window's contents only,
        // and fill it with the initial background color.
        let mut framebuffer = Framebuffer::new(width, height, None)?;
        framebuffer.fill(initial_background.into());
        let (width, height) = framebuffer.get_size();

        // TODO: FIXME: (kevinaboos) this condition seems wrong... at least the first conditional does.
        if width <= 2 * DEFAULT_TITLE_BAR_HEIGHT || height <= DEFAULT_TITLE_BAR_HEIGHT + DEFAULT_BORDER_SIZE {
            return Err("window dimensions must be large enough for the title bar and borders to be drawn");
        }

        // Create an event queue to allow the window manager to pass events to this `Window` via its `WindowInner` instance,
        // and to allow applications to receive events from this `Window` object itself.
        let event_consumer = Queue::with_capacity(100);
        let event_producer = event_consumer.clone();

        let window_inner = WindowInner::new(coordinate, framebuffer, event_producer);
        let mut window = Window {
            inner: Arc::new(Mutex::new(window_inner)),
            event_consumer,
            last_mouse_position_event: MousePositionEvent::default(),
            last_is_active: true, // new window is now set as the active window by default 
        };

        // Draw the actual window frame, the title bar and borders.
        window.draw_border(true);
        {
            let mut inner = window.inner.lock();
            window.show_button(TopButton::Close, 1, &mut inner);
            window.show_button(TopButton::MinimizeMaximize, 1, &mut inner);
            window.show_button(TopButton::Hide, 1, &mut inner);
        }

        let _window_bounding_box = Rectangle {
            top_left: coordinate,
            bottom_right: coordinate + (width as isize, height as isize)
        };

        let mut wm = wm_ref.lock();
        wm.set_active(&window.inner, false)?; 

        // Currently, refresh the whole screen instead of just the new window's bounds
        // wm.refresh_bottom_windows(Some(window_bounding_box), true)?;
        wm.refresh_bottom_windows(Option::<Rectangle>::None, true)?;
        
        Ok(window)
    }


    /// Tries to receive an `Event` that has been sent to this `Window`.
    /// If no events exist on the queue, it returns `Ok(None)`. 
    /// 
    /// "Internal" events will be automatically handled rather than returned. 
    /// If an error occurs while obtaining the event (or when handling internal events),
    ///
    /// Otherwise, the event at the front of this window's event queue will be popped off and returned. 
    pub fn handle_event(&mut self) -> Result<Option<Event>, &'static str> {
        let mut call_later_do_refresh_floating_border = false;
        let mut call_later_do_move_active_window = false;
        let mut need_to_set_active = false;
        let mut need_refresh_three_button = false;

        let wm_ref = window_manager::WINDOW_MANAGER.get().ok_or("The window manager is not initialized")?;
        
        let is_active = {
            let wm = wm_ref.lock();
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

        // If we cannot handle this event as an "internal" event (e.g., clicking on the window title bar or border),
        // we simply return that event from this function such that the application can handle it. 
        let mut unhandled_event: Option<Event> = None;

        
        while let Some(event) = self.event_consumer.pop() {
            // TODO FIXME: for a performant design, the goal is to AVOID holding the lock on `inner` as much as possible. 
            //             That means that most of the drawing logic should be moved into the `window_inner` crate itself.
            let mut inner = self.inner.lock();
            let (width, height) = inner.get_size();

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
                                // Thus, we let the caller handle it.
                                unhandled_event = Some(Event::MousePositionEvent(mouse_event.clone()));
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
                    unhandled_event = Some(unhandled);
                }
            }

            // Immediately return any unhandled events to the caller
            // before we loop back to handle additional events.
            if unhandled_event.is_some() {
                break;
            }
        }

        let mut wm = wm_ref.lock();
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

        Ok(unhandled_event)
    }

    /// Renders the area of this `Window` specified by the given `bounding_box`,
    /// which is relative to the top-left coordinate of this `Window`.
    /// 
    /// Refreshes the whole window if `bounding_box` is `None`.
    /// 
    /// This method should be invoked after updating the window's contents in order to see its new content.
    pub fn render(&mut self, bounding_box: Option<Rectangle>) -> Result<(), &'static str> {

        // Induced bug rendering attempting to access out of bound memory
        #[cfg(downtime_eval)]
        {
            if bounding_box.unwrap().top_left == Coord::new(150,150) {
                unsafe { *(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555; }
            }
        }

        let wm_ref = WINDOW_MANAGER.get().ok_or("The static window manager was not yet initialized")?;

        // Convert the given relative `bounding_box` to an absolute one (relative to the screen, not the window).
        let coordinate = {
            let window = self.inner.lock();
            window.get_position()
        };
        let absolute_bounding_box = bounding_box.map(|bb| bb + coordinate);

        wm_ref.lock().refresh_windows(absolute_bounding_box)
    }

    /// Returns a `Rectangle` describing the position and dimensions of this Window's content region,
    /// i.e., the area within the window excluding the title bar and border
    /// that is available for rendering application content. 
    /// 
    /// The returned `Rectangle` is expressed relative to this Window's position.
    pub fn area(&self) -> Rectangle {
        self.inner.lock().content_area()
    }

    /// Returns an immutable reference to this window's virtual `Framebuffer`. 
    pub fn framebuffer(&self) -> MutexGuardRef<WindowInner, Framebuffer<AlphaPixel>> {
        MutexGuardRef::new(self.inner.lock()).map(|inner| inner.framebuffer())
    }

    /// Returns a mutable reference to this window's virtual `Framebuffer`. 
    pub fn framebuffer_mut(&mut self) -> MutexGuardRefMut<WindowInner, Framebuffer<AlphaPixel>> {
        MutexGuardRefMut::new(self.inner.lock()).map_mut(|inner| inner.framebuffer_mut())
    }

    /// Returns `true` if this window is the currently active window. 
    /// 
    /// Obtains the lock on the window manager instance. 
    pub fn is_active(&self) -> bool {
        WINDOW_MANAGER.get()
            .map(|wm| wm.lock().is_active(&self.inner))
            .unwrap_or(false)
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
            inner.framebuffer_mut(),
            Coord::new(0, title_bar_height as isize),
            border_size,
            height - title_bar_height,
            border_color.into(),
        );

        framebuffer_drawer::draw_rectangle(
            inner.framebuffer_mut(),
            Coord::new(0, (height - border_size) as isize),
            width,
            border_size,
            border_color.into(),
        );
        framebuffer_drawer::draw_rectangle(
            inner.framebuffer_mut(),
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
                    inner.framebuffer_mut(),
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
                inner.framebuffer_mut(),
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
                    inner.framebuffer_mut().overwrite_pixel(Coord::new(i as isize, j as isize), trans_pixel);
                    inner.framebuffer_mut().overwrite_pixel(Coord::new((width - i - 1) as isize, j as isize), trans_pixel);
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
            inner.framebuffer_mut(),
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
        if let Some(wm) = WINDOW_MANAGER.get() {
            if let Err(err) = wm.lock().delete_window(&self.inner) {
                error!("Failed to delete_window upon drop: {:?}", err);
            }
        } else {
            error!("BUG: Could not delete_window upon drop because the window manager was not initialized");
        }
    }
}