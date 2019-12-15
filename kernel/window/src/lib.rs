//! A `Window` object should be owned by an application. It contains a list of displayables which can display in the window. See `applications/new_window` as a demo to use this library.
//!
//! This library will create a window with default title bar and border. It handles the commonly used interactions like moving
//! the window or close the window. Also, it is responsible to show title bar differently when window is active. 
//!
//! A window can render itself to the screen via a window manager. The window manager will compute the updated area and composites it with other existing windows according to their order.
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
extern crate compositor;
extern crate displayable;
extern crate font;
extern crate frame_buffer;
extern crate frame_buffer_alpha;
extern crate frame_buffer_compositor;
extern crate frame_buffer_drawer;
extern crate memory_structs;
extern crate mouse;
extern crate window_inner;
extern crate window_manager;
extern crate shapes;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use compositor::{Compositor, FrameBufferUpdates};
use core::ops::Deref;
use core::ops::DerefMut;
use mpmc::Queue;
use displayable::Displayable;
use event_types::{Event, MousePositionEvent};
use frame_buffer::{FrameBuffer, Pixel, AlphaPixel, pixel::{BLACK, PixelColor}};
use shapes::{Coord, Rectangle};
use frame_buffer_compositor::{FRAME_COMPOSITOR};
use memory_structs::PhysicalAddress;
use spin::Mutex;
use window_inner::WindowInner;
use window_manager::{WINDOW_MANAGER, WindowManager};

// The title bar size, in number of pixels
const WINDOW_TITLE_BAR: usize = 15;
// left, right, bottom border size, in number of pixels
const WINDOW_BORDER: usize = 2;
// border radius, in number of pixels
const WINDOW_RADIUS: usize = 5;
// border and title bar color when window is inactive
const WINDOW_BORDER_COLOR_INACTIVE: PixelColor = 0x00333333;
// border and title bar color when window is active, the top part color
const WINDOW_BORDER_COLOR_ACTIVE_TOP: PixelColor = 0x00BBBBBB;
// border and title bar color when window is active, the bottom part color
const WINDOW_BORDER_COLOR_ACTIVE_BOTTOM: PixelColor = 0x00666666;
// window button color: red
const WINDOW_BUTTON_COLOR_CLOSE: PixelColor = 0x00E74C3C;
// window button color: green
const WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE: PixelColor = 0x00239B56;
// window button color: purple
const WINDOW_BUTTON_COLOR_HIDE: PixelColor = 0x007D3C98;
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

/// Abstraction of a window which owns a list of components and the window's handler. It provides title bar which helps user moving, close, maximize or minimize window
pub struct Window<T: Pixel + Copy> {
    /// this object contains states and methods related to display the window on the screen
    pub inner: Arc<Mutex<WindowInner<T>>>,
    /// the width of border, init as WINDOW_BORDER. the border is still part of the window and remains flexibility for user to change border style or remove border. However, for most application a border is useful for user to identify the region.
    border_size: usize,
    /// the height of title bar in pixel, init as WINDOW_TITLE_BAR. it is render inside the window so user shouldn't use this area anymore
    title_size: usize,
    /// the background of this window, init as WINDOW_BACKGROUND
    background: PixelColor,
    /// application could get events from this consumer
    pub consumer: Queue<Event>,
    /// event output used by window manager, private variable
    producer: Queue<Event>,
    /// last mouse position event, used to judge click and press-moving event
    last_mouse_position_event: MousePositionEvent,
    /// record last result of whether this window is active, to reduce redraw overhead
    last_is_active: bool,
    // /// The displayable in the window as components.
    // components: BTreeMap<String, Component>,
}

impl<T: Pixel + Copy> Window<T> {
    /// create new Window by given position and size, return the Mutex of it for ease of sharing
    /// x, y is the distance in pixel relative to top-left of window
    pub fn new(
        coordinate: Coord,
        width: usize,
        height: usize,
        background: u32,
        wm_mutex: &Mutex<WindowManager<T>>
    ) -> Result<Window<T>, &'static str> {
        let framebuffer: FrameBuffer<T> = FrameBuffer::new(width, height, None)?;
        let (width, height) = framebuffer.get_size();
        if width <= 2 * WINDOW_TITLE_BAR || height <= WINDOW_TITLE_BAR + WINDOW_BORDER {
            return Err("window too small to even draw border");
        }

        let inner_mutex = window_inner::new_window(coordinate, framebuffer)?;

        // create event queue for components
        let consumer = Queue::with_capacity(100);
        let producer = consumer.clone();

        let mut window = Window {
            inner: inner_mutex,
            border_size: WINDOW_BORDER,
            title_size: WINDOW_TITLE_BAR,
            background: background,
            consumer: consumer,
            producer: producer,
            last_mouse_position_event: MousePositionEvent::default(),
            last_is_active: true, // new window is by default active
            //components: BTreeMap::new(),
        };

        {
            let mut inner = window.inner.lock();
            inner.framebuffer.fill_color(T::from(window.background));
        }

        {
            let mut wm = wm_mutex.lock();
            wm.set_active(&window.inner, false)?; // do not refresh now for
        }

        window.draw_border(true); // draw window with active border
                                    // draw three buttons
        {
            let mut inner = window.inner.lock();
            window.show_button(TopButton::Close, 1, &mut inner);
            window.show_button(TopButton::MinimizeMaximize, 1, &mut inner);
            window.show_button(TopButton::Hide, 1, &mut inner);
            let buffer_blocks: FrameBufferUpdates<T> = FrameBufferUpdates {
                framebuffer: &inner.framebuffer,
                coordinate: coordinate,
            };

            let area = Rectangle {
                top_left: coordinate,
                bottom_right: coordinate + (width as isize, height as isize)
            };

            // Wenqiu: use WM
            let mut wm = wm_mutex.lock();
            FRAME_COMPOSITOR.lock().composite(Some(buffer_blocks), &mut wm.final_fb, Some(area))?;
        }

        Ok(window)
    }

/*    /// Add a new displayable to the window at the coordinate relative to the top-left of the window.
    pub fn add_displayable(
        &mut self,
        key: &str,
        coordinate: Coord,
        displayable: Box<dyn Displayable>,
    ) -> Result<(), &'static str> {
        let key = key.to_string();
        let component = Component {
            coordinate: coordinate + self.inner_position(),
            displayable: displayable,
        };
        self.components.insert(key, component);
        Ok(())
    }

    /// Gets the position of a displayable relative to the top-left of the window
    pub fn get_displayable_position(&self, key: &str) -> Result<Coord, &'static str> {
        let opt = self.components.get(key);
        match opt {
            None => {
                return Err("No such displayable");
            }
            Some(component) => {
                return Ok(component.get_position());
            }
        };
    }

    /// Gets a reference to a displayable of type `T` which implements the `Displayable` trait by its name. Returns error if the displayable is not of type `T` or does not exist.
    pub fn get_concrete_display<U: Displayable>(
        &self,
        display_name: &str,
    ) -> Result<&U, &'static str> {
        if let Some(component) = self.components.get(display_name) {
            if let Some(display) = component.displayable.downcast_ref::<U>() {
                return Ok(display);
            } else {
                return Err("The displayable is not of this type");
            }
        } else {
            return Err("The displayable does not exist");
        }
    }

    /// Gets a reference to a displayable of type `T` which implements the `Displayable` trait by its name. Returns error if the displayable is not of type `T` or does not exist.
    pub fn get_concrete_display_mut<U: Displayable>(
        &mut self,
        display_name: &str,
    ) -> Result<&mut U, &'static str> {
        if let Some(component) = self.components.get_mut(display_name) {
            if let Some(display) = component.displayable.downcast_mut::<U>() {
                return Ok(display);
            } else {
                return Err("The displayable is not of this type");
            }
        } else {
            return Err("The displayable does not exist");
        }
    }
*/
    /// Display a displayable by its name.
    pub fn display(&mut self, displayable: &mut Displayable<T>, coordinate: Coord) -> Result<(), &'static str> {
        // let component = self.components.get_mut(display_name).ok_or("The displayable does not exist")?;
        // let coordinate = component.get_position();
        let area = {
            let mut window = self.inner.lock();
            let area = displayable.display(
                coordinate + self.inner_position(), 
                &mut window.framebuffer
            )?;
            area
        };

        self.render(Some(area))
    }

    /// Handles the event sent to the window by window manager
    pub fn handle_event(&mut self, wm_mut: &Mutex<WindowManager<T>>) -> Result<(), &'static str> {
        let mut call_later_do_refresh_floating_border = false;
        let mut call_later_do_move_active_window = false;
        let mut need_to_set_active = false;
        let mut need_refresh_three_button = false;

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
            let consumer = &inner.consumer;
            let event = match consumer.pop() {
                Some(ev) => ev,
                _ => {
                    break;
                    //return Ok(());
                }
            };

            match event {
                Event::KeyboardEvent(ref input_event) => {
                    let key_input = input_event.key_event;
                    self.producer.push(Event::new_keyboard_event(key_input)).map_err(|_e| "Fail to push the keyboard event")?;
                }
                Event::MousePositionEvent(ref mouse_event) => {
                    if inner.is_moving {
                        // only wait for left button up to exit this mode
                        if !mouse_event.left_button_hold {
                            inner.is_moving = false;
                            self.last_mouse_position_event = mouse_event.clone();
                            call_later_do_move_active_window = true;
                        }
                        call_later_do_refresh_floating_border = true;
                    } else {
                        if (mouse_event.coordinate.y as usize) < self.title_size
                            && (mouse_event.coordinate.x as usize) < inner.width
                        {
                            // the region of title bar
                            let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
                            let mut is_three_button = false;
                            for i in 0..3 {
                                let dcoordinate = Coord::new(
                                    mouse_event.coordinate.x
                                        - WINDOW_BUTTON_BIAS_X as isize
                                        - (i as isize) * WINDOW_BUTTON_BETWEEN as isize,
                                    mouse_event.coordinate.y - self.title_size as isize / 2,
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
                                            // click event
                                            if i == 0 {
                                                debug!("close window");
                                                return Err("user close window");
                                                // window will not close until app drop self
                                            }
                                        }
                                    }
                                } else {
                                    self.show_button(TopButton::from(i), 1, &mut inner);
                                    need_refresh_three_button = true;
                                }
                            }
                            // check if user push the title bar, which means user willing to move the window
                            if !is_three_button
                                && !self.last_mouse_position_event.left_button_hold
                                && mouse_event.left_button_hold
                            {
                                inner.is_moving = true;
                                inner.moving_base = mouse_event.gcoordinate;
                                call_later_do_refresh_floating_border = true;
                            }
                        } else {
                            // the region of components
                            // TODO: if any components want this event? ask them!
                            self.producer
                                .push(Event::MousePositionEvent(mouse_event.clone())).map_err(|_e| "Fail to push the keyboard event")?;
                        }
                        if (mouse_event.coordinate.y as usize) < inner.height
                            && (mouse_event.coordinate.x as usize) < inner.width
                            && !self.last_mouse_position_event.left_button_hold
                            && mouse_event.left_button_hold
                        {
                            need_to_set_active = true;
                        }
                        self.last_mouse_position_event = mouse_event.clone();
                    }
                }
                _ => {
                    return Ok(());
                }
            };
            // event.mark_completed();
        }

        // Wenqiu: optimize
        if need_refresh_three_button {
            let mut wm = wm_mut.lock();
            self.refresh_three_button(wm.deref_mut())?;
        }

        let mut wm = wm_mut.lock();
        if need_to_set_active {
            wm.set_active(&self.inner, true)?;
        }

        if call_later_do_refresh_floating_border {
            wm.move_floating_border()?;
        }

        if call_later_do_move_active_window {
            wm.move_active_window()?;
        }

        Ok(())
    }

    /// Render a window to the screen. Should be invoked after updating.
    pub fn render(&mut self, area: Option<Rectangle>) -> Result<(), &'static str> {
        let coordinate = {
            let window = self.inner.lock();
            window.get_position()
        };

        let mut wm = WINDOW_MANAGER
            .try()
            .ok_or("The static window manager was not yet initialized")?
            .lock();

        let absolute_area = match area {
            Some(area) => Some(area + coordinate),
            None => None,
        };
        wm.refresh_windows(absolute_area, true)
    }

    /// Draw the border of this window, with argument of whether this window is active now
    fn draw_border(&mut self, active: bool) {
        let mut inner = self.inner.lock();
        // first draw left, bottom, right border
        let mut border_color = WINDOW_BORDER_COLOR_INACTIVE;
        if active {
            border_color = WINDOW_BORDER_COLOR_ACTIVE_BOTTOM;
        }
        let width = inner.width;
        let height = inner.height;

        frame_buffer_drawer::draw_rectangle(
            &mut inner.framebuffer,
            Coord::new(0, self.title_size as isize),
            self.border_size,
            height - self.title_size,
            border_color,
        );

        frame_buffer_drawer::draw_rectangle(
            &mut inner.framebuffer,
            Coord::new(0, (height - self.border_size) as isize),
            width,
            self.border_size,
            border_color,
        );
        frame_buffer_drawer::draw_rectangle(
            &mut inner.framebuffer,
            Coord::new(
                (width - self.border_size) as isize,
                self.title_size as isize,
            ),
            self.border_size,
            height - self.title_size,
            border_color,
        );

        // then draw the title bar
        if active {
            for i in 0..self.title_size {
                frame_buffer_drawer::draw_rectangle(
                    &mut inner.framebuffer,
                    Coord::new(0, i as isize),
                    width,
                    1,
                    T::from(WINDOW_BORDER_COLOR_ACTIVE_BOTTOM).color_mix(
                        T::from(WINDOW_BORDER_COLOR_ACTIVE_TOP),
                        (i as f32) / (self.title_size as f32),
                    ).color(),
                ); 
            }
        } else {
            frame_buffer_drawer::draw_rectangle(
                &mut inner.framebuffer,
                Coord::new(0, 0),
                width,
                self.title_size,
                border_color,
            );
        }

        // draw radius finally
        let r2 = WINDOW_RADIUS * WINDOW_RADIUS;
        for i in 0..WINDOW_RADIUS {
            for j in 0..WINDOW_RADIUS {
                let dx1 = WINDOW_RADIUS - i;
                let dy1 = WINDOW_RADIUS - j;
                if dx1 * dx1 + dy1 * dy1 > r2 {
                    // draw this to transparent
                    inner.framebuffer
                        .overwrite_pixel(Coord::new(i as isize, j as isize), T::from(0xFFFFFFFF));
                    inner.framebuffer.overwrite_pixel(
                        Coord::new((width - i - 1) as isize, j as isize),
                        T::from(0xFFFFFFFF),
                    );
                }
            }
        }
    }

    /// show three button with status. state = 0,1,2 for three different color
    fn show_button(&self, button: TopButton, state: usize, inner: &mut WindowInner<T>) {
        let y = self.title_size / 2;
        let x = WINDOW_BUTTON_BIAS_X
            + WINDOW_BUTTON_BETWEEN
                * match button {
                    TopButton::Close => 0,
                    TopButton::MinimizeMaximize => 1,
                    TopButton::Hide => 2,
                };
        frame_buffer_drawer::draw_circle(
            &mut inner.framebuffer,
            Coord::new(x as isize, y as isize),
            WINDOW_BUTTON_SIZE,
            T::from(BLACK).color_mix(
                T::from(match button {
                    TopButton::Close => WINDOW_BUTTON_COLOR_CLOSE,
                    TopButton::MinimizeMaximize => WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE,
                    TopButton::Hide => WINDOW_BUTTON_COLOR_HIDE,
                }),
                0.2f32 * (state as f32),
            ).color(),
        );
    }

    /// refresh the top left three button's appearance
    fn refresh_three_button(&self, wm: &mut WindowManager<T>) -> Result<(), &'static str> {
        let inner = self.inner.lock();
        let width = inner.get_size().0;

        let frame_buffer_blocks = FrameBufferUpdates {
            framebuffer: &inner.framebuffer,
            coordinate: inner.coordinate,
        };

        let update_area = Rectangle {
            top_left: Coord::new(0, 0),
            bottom_right: Coord::new(width as isize, self.title_size as isize)
        };

        FRAME_COMPOSITOR.lock().composite(Some(frame_buffer_blocks), &mut wm.final_fb, Some(update_area))?;

        Ok(())
    }

    /// return the available inner size, excluding title bar and border
    pub fn inner_size(&self) -> (usize, usize) {
        let inner = self.inner.lock();
        (
            inner.width - 2 * self.border_size,
            inner.height - self.border_size - self.title_size,
        )
    }

    /// return the top-left coordinate of the available inner position, excluding title bar and border
    pub fn inner_position(&self) -> Coord {
        Coord::new(self.border_size as isize, self.title_size as isize)
    }

    /// get space remained for border, in number of pixel. There is border on the left, right and bottom.
    /// When user add their components, should margin its area to avoid overlapping these borders.
    pub fn get_border_size(&self) -> usize {
        self.border_size
    }

    /// get space remained for title bar, in number of pixel. The title bar is on the top of the window, so when user
    /// add their components, should margin its area to avoid overlapping the title bar.
    pub fn get_title_size(&self) -> usize {
        self.title_size
    }
}

impl<T: Pixel + Copy> Drop for Window<T> {
    fn drop(&mut self) {
// Wenqiu: remove

        // match WINDOW_MANAGER
        //     .try()
        //     .ok_or("The static window manager was not yet initialized")
        // {
        //     Ok(wm) => {
        //         if let Err(err) = wm.lock().delete_window(&self.inner) {
        //             error!("delete_window failed {}", err);
        //         }
        //     }
        //     Err(err) => {
        //         error!("delete_window failed {}", err);
        //     }
        // }
    }
}

// /// A component contains a displayable and its coordinate relative to the top-left corner of the window.
// struct Component {
//     coordinate: Coord,
//     displayable: Box<dyn Displayable>,
// }

// impl Component {
//     /// gets the coordinate of the displayable relative to the top-left corner of the window
//     fn get_position(&self) -> Coord {
//         self.coordinate
//     }
// }
