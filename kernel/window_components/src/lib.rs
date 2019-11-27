//! This Window Components is designated to help user to build easy-to-use GUI applications
//!
//! A `WindowComponent` object should be owned by application. It contains a list of displayables which can display in the window. See `applications/new_window` as a demo to use this library
//!
//! This library will create a window with default title bar and border. It handles the commonly used interactions like moving
//! the window or close the window. Also, it is responsible to show title bar differently when window is active. The library
//! frees applications from handling the complicated interaction with window manager, however, advanced users could learn from
//! this library about how to use window manager APIs directly.
//!

#![no_std]
#[macro_use]
extern crate alloc;
extern crate dfqueue;
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
extern crate window;
extern crate window_generic;
extern crate window_manager;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use compositor::Compositor;
use core::ops::Deref;
use core::ops::DerefMut;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use displayable::Displayable;
use event_types::{Event, MousePositionEvent};
use frame_buffer::{Coord, FrameBuffer, Pixel, RectArea};
use frame_buffer_alpha::{PixelMixer, BLACK};
use frame_buffer_compositor::{FrameBufferBlocks, FRAME_COMPOSITOR};
use memory_structs::PhysicalAddress;
use spin::Mutex;
use window::Window;
use window_generic::WindowGeneric;
use window_manager::WINDOW_MANAGER;

// The title bar size, in number of pixels
const WINDOW_TITLE_BAR: usize = 15;
// left, right, bottom border size, in number of pixels
const WINDOW_BORDER: usize = 2;
// border radius, in number of pixels
const WINDOW_RADIUS: usize = 5;
// border and title bar color when window is inactive
const WINDOW_BORDER_COLOR_INACTIVE: Pixel = 0x00333333;
// border and title bar color when window is active, the top part color
const WINDOW_BORDER_COLOR_ACTIVE_TOP: Pixel = 0x00BBBBBB;
// border and title bar color when window is active, the bottom part color
const WINDOW_BORDER_COLOR_ACTIVE_BOTTOM: Pixel = 0x00666666;
// window button color: red
const WINDOW_BUTTON_COLOR_CLOSE: Pixel = 0x00E74C3C;
// window button color: green
const WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE: Pixel = 0x00239B56;
// window button color: purple
const WINDOW_BUTTON_COLOR_HIDE: Pixel = 0x007D3C98;
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

/// Abstract of a window, a list of components and the window's handler, providing title bar which helps user moving, close, maximize or minimize window
pub struct WindowComponents {
    /// the window object that could be used to initialize components
    pub winobj: Arc<Mutex<WindowGeneric>>,
    /// the width of border, init as WINDOW_BORDER. the border is still part of the window and remains flexibility for user to change border style or remove border. However, for most application a border is useful for user to identify the region.
    border_size: usize,
    /// the height of title bar in pixel, init as WINDOW_TITLE_BAR. it is render inside the window so user shouldn't use this area anymore
    title_size: usize,
    /// the background of this window, init as WINDOW_BACKGROUND
    background: Pixel,
    /// application could get events from this consumer
    pub consumer: DFQueueConsumer<Event>,
    /// event output used by window manager, private variable
    producer: DFQueueProducer<Event>,
    /// last mouse position event, used to judge click and press-moving event
    last_mouse_position_event: MousePositionEvent,
    /// record last result of whether this window is active, to reduce redraw overhead
    last_is_active: bool,
    /// The displayable in the window as components.
    components: BTreeMap<String, Component>,
}

impl WindowComponents {
    /// create new WindowComponents by given position and size, return the Mutex of it for ease of sharing
    /// x, y is the distance in pixel relative to top-left of window
    pub fn new(
        coordinate: Coord,
        width: usize,
        height: usize,
        background: u32,
        new_framebuffer: &dyn Fn(
            usize,
            usize,
            Option<PhysicalAddress>,
        ) -> Result<Box<dyn FrameBuffer>, &'static str>,
    ) -> Result<WindowComponents, &'static str> {
        let framebuffer = new_framebuffer(width, height, None)?;
        let (width, height) = framebuffer.get_size();
        if width <= 2 * WINDOW_TITLE_BAR || height <= WINDOW_TITLE_BAR + WINDOW_BORDER {
            return Err("window too small to even draw border");
        }

        let winobj_mutex = window_generic::new_window(coordinate, framebuffer)?;

        // create event queue for components
        let consumer = DFQueue::new().into_consumer();
        let producer = consumer.obtain_producer();

        let mut wincomps: WindowComponents = WindowComponents {
            winobj: winobj_mutex,
            border_size: WINDOW_BORDER,
            title_size: WINDOW_TITLE_BAR,
            background: background,
            consumer: consumer,
            producer: producer,
            last_mouse_position_event: MousePositionEvent {
                coordinate: Coord::new(0, 0),
                gcoordinate: Coord::new(0, 0),
                scrolling_up: false,
                scrolling_down: false,
                left_button_hold: false,
                right_button_hold: false,
                fourth_button_hold: false,
                fifth_button_hold: false,
            },
            last_is_active: true, // new window is by default active
            components: BTreeMap::new(),
        };

        {
            let mut winobj = wincomps.winobj.lock();
            winobj.framebuffer.fill_color(wincomps.background);
        }

        {
            let mut win = WINDOW_MANAGER
                .try()
                .ok_or("The static window manager was not yet initialized")?
                .lock();
            win.set_active(&wincomps.winobj, false)?; // do not refresh now for
        }

        wincomps.draw_border(true); // draw window with active border
                                    // draw three buttons
        {
            let mut winobj = wincomps.winobj.lock();
            wincomps.show_button(TopButton::Close, 1, &mut winobj);
            wincomps.show_button(TopButton::MinimizeMaximize, 1, &mut winobj);
            wincomps.show_button(TopButton::Hide, 1, &mut winobj);
            let buffer_blocks = FrameBufferBlocks {
                framebuffer: winobj.framebuffer.deref(),
                coordinate: coordinate,
                blocks: None,
            };

            FRAME_COMPOSITOR
                .lock()
                .composite(vec![buffer_blocks].into_iter())?;
        }

        Ok(wincomps)
    }

    /// Add a new displayable to the window at the coordinate relative to the top-left of the window.
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

    /// Clear a displayable in the window.
    pub fn clear_displayable(&mut self, display_name: &str) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let coordinate = component.get_position();

        {
            let mut window = self.winobj.lock();
            component
                .displayable
                .clear(coordinate, Some(window.framebuffer_mut()))?;
        }

        self.render(None)
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
    pub fn get_concrete_display<T: Displayable>(
        &self,
        display_name: &str,
    ) -> Result<&T, &'static str> {
        if let Some(component) = self.components.get(display_name) {
            if let Some(display) = component.displayable.downcast_ref::<T>() {
                return Ok(display);
            } else {
                return Err("The displayable is not of this type");
            }
        } else {
            return Err("The displayable does not exist");
        }
    }

    /// Gets a reference to a displayable of type `T` which implements the `Displayable` trait by its name. Returns error if the displayable is not of type `T` or does not exist.
    pub fn get_concrete_display_mut<T: Displayable>(
        &mut self,
        display_name: &str,
    ) -> Result<&mut T, &'static str> {
        if let Some(component) = self.components.get_mut(display_name) {
            if let Some(display) = component.displayable.downcast_mut::<T>() {
                return Ok(display);
            } else {
                return Err("The displayable is not of this type");
            }
        } else {
            return Err("The displayable does not exist");
        }
    }

    /// Display a displayable by its name.
    pub fn display(&mut self, display_name: &str) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let coordinate = component.get_position();

        let area = {
            let mut window = self.winobj.lock();
            let area = component
                .displayable
                .display(coordinate, Some(window.framebuffer_mut()))?;
            area
        };

        self.render(Some(area))
    }

    /// Handles the event sent to the window by window manager
    pub fn handle_event(&mut self) -> Result<(), &'static str> {
        let mut call_later_do_refresh_floating_border = false;
        let mut call_later_do_move_active_window = false;
        let mut need_to_set_active = false;
        let mut need_refresh_three_button = false;

        let is_active = {
            let wm = window_manager::WINDOW_MANAGER
                .try()
                .ok_or("The static window manager was not yet initialized")?
                .lock();
            wm.is_active(&self.winobj)
        };
        if is_active != self.last_is_active {
            self.draw_border(is_active);
            self.last_is_active = is_active;
            let mut winobj = self.winobj.lock();
            self.show_button(TopButton::Close, 1, &mut winobj);
            self.show_button(TopButton::MinimizeMaximize, 1, &mut winobj);
            self.show_button(TopButton::Hide, 1, &mut winobj);
        }

        {
            let mut winobj = self.winobj.lock();
            let consumer = &winobj.consumer;
            let event = match consumer.peek() {
                Some(ev) => ev,
                _ => {
                    return Ok(());
                }
            };

            match event.deref() {
                &Event::KeyboardEvent(ref input_event) => {
                    let key_input = input_event.key_event;
                    self.producer.enqueue(Event::new_keyboard_event(key_input));
                }
                &Event::MousePositionEvent(ref mouse_event) => {
                    if winobj.is_moving() {
                        // only wait for left button up to exit this mode
                        if !mouse_event.left_button_hold {
                            winobj.set_is_moving(false);
                            winobj.set_give_all_mouse_event(false);
                            self.last_mouse_position_event = mouse_event.clone();
                            call_later_do_refresh_floating_border = true;
                            call_later_do_move_active_window = true;
                        }
                    } else {
                        if (mouse_event.coordinate.y as usize) < self.title_size
                            && (mouse_event.coordinate.x as usize) < winobj.width
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
                                        self.show_button(TopButton::from(i), 2, &mut winobj);
                                        need_refresh_three_button = true;
                                    } else {
                                        self.show_button(TopButton::from(i), 0, &mut winobj);
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
                                    self.show_button(TopButton::from(i), 1, &mut winobj);
                                    need_refresh_three_button = true;
                                }
                            }
                            // check if user push the title bar, which means user willing to move the window
                            if !is_three_button
                                && !self.last_mouse_position_event.left_button_hold
                                && mouse_event.left_button_hold
                            {
                                winobj.set_is_moving(true);
                                winobj.set_give_all_mouse_event(true);
                                winobj.moving_base = mouse_event.gcoordinate;
                                call_later_do_refresh_floating_border = true;
                            }
                        } else {
                            // the region of components
                            // TODO: if any components want this event? ask them!
                            self.producer
                                .enqueue(Event::MousePositionEvent(mouse_event.clone()));
                        }
                        if (mouse_event.coordinate.y as usize) < winobj.height
                            && (mouse_event.coordinate.x as usize) < winobj.width
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
            event.mark_completed();
        }

        if need_refresh_three_button {
            self.refresh_three_button()?;
        }

        let mut wm = WINDOW_MANAGER
            .try()
            .ok_or("The static window manager was not yet initialized")?
            .lock();
        if need_to_set_active {
            wm.set_active(&self.winobj, true)?;
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
    pub fn render(&mut self, area: Option<RectArea>) -> Result<(), &'static str> {
        let coordinate = {
            let window = self.winobj.lock();
            window.get_position()
        };

        let wm = WINDOW_MANAGER
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
        let mut winobj = self.winobj.lock();
        // first draw left, bottom, right border
        let mut border_color = WINDOW_BORDER_COLOR_INACTIVE;
        if active {
            border_color = WINDOW_BORDER_COLOR_ACTIVE_BOTTOM;
        }
        let width = winobj.width;
        let height = winobj.height;

        frame_buffer_drawer::draw_rectangle(
            winobj.framebuffer.deref_mut(),
            Coord::new(0, self.title_size as isize),
            self.border_size,
            height - self.title_size,
            border_color,
        );

        frame_buffer_drawer::draw_rectangle(
            winobj.framebuffer.deref_mut(),
            Coord::new(0, (height - self.border_size) as isize),
            width,
            self.border_size,
            border_color,
        );
        frame_buffer_drawer::draw_rectangle(
            winobj.framebuffer.deref_mut(),
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
                    winobj.framebuffer.deref_mut(),
                    Coord::new(0, i as isize),
                    width,
                    1,
                    WINDOW_BORDER_COLOR_ACTIVE_BOTTOM.color_mix(
                        WINDOW_BORDER_COLOR_ACTIVE_TOP,
                        (i as f32) / (self.title_size as f32),
                    ),
                ); 
            }
        } else {
            frame_buffer_drawer::draw_rectangle(
                winobj.framebuffer.deref_mut(),
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
                    winobj
                        .framebuffer
                        .overwrite_pixel(Coord::new(i as isize, j as isize), 0xFFFFFFFF);
                    winobj.framebuffer.overwrite_pixel(
                        Coord::new((width - i - 1) as isize, j as isize),
                        0xFFFFFFFF,
                    );
                }
            }
        }
    }

    /// show three button with status. state = 0,1,2 for three different color
    fn show_button(&self, button: TopButton, state: usize, winobj: &mut WindowGeneric) {
        let y = self.title_size / 2;
        let x = WINDOW_BUTTON_BIAS_X
            + WINDOW_BUTTON_BETWEEN
                * match button {
                    TopButton::Close => 0,
                    TopButton::MinimizeMaximize => 1,
                    TopButton::Hide => 2,
                };
        frame_buffer_drawer::draw_circle(
            winobj.framebuffer.deref_mut(),
            Coord::new(x as isize, y as isize),
            WINDOW_BUTTON_SIZE,
            BLACK.color_mix(
                match button {
                    TopButton::Close => WINDOW_BUTTON_COLOR_CLOSE,
                    TopButton::MinimizeMaximize => WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE,
                    TopButton::Hide => WINDOW_BUTTON_COLOR_HIDE,
                },
                0.2f32 * (state as f32),
            ),
        );
    }

    /// refresh the top left three button's appearance
    fn refresh_three_button(&self) -> Result<(), &'static str> {
        let profile = self.winobj.lock();
        let frame_buffer_blocks = FrameBufferBlocks {
            framebuffer: profile.framebuffer.deref(),
            coordinate: profile.coordinate,
            blocks: None,
        };
        FRAME_COMPOSITOR
            .lock()
            .composite(vec![frame_buffer_blocks].into_iter())?;

        Ok(())
    }

    /// return the available inner size, excluding title bar and border
    pub fn inner_size(&self) -> (usize, usize) {
        let winobj = self.winobj.lock();
        (
            winobj.width - 2 * self.border_size,
            winobj.height - self.border_size - self.title_size,
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

impl Drop for WindowComponents {
    fn drop(&mut self) {
        match WINDOW_MANAGER
            .try()
            .ok_or("The static window manager was not yet initialized")
        {
            Ok(wm) => {
                if let Err(err) = wm.lock().delete_window(&self.winobj) {
                    error!("delete_window failed {}", err);
                }
            }
            Err(err) => {
                error!("delete_window failed {}", err);
            }
        }
    }
}

/// A component contains a displayable and its coordinate relative to the top-left corner of the window.
struct Component {
    coordinate: Coord,
    displayable: Box<dyn Displayable>,
}

impl Component {
    /// gets the coordinate of the displayable relative to the top-left corner of the window
    fn get_position(&self) -> Coord {
        self.coordinate
    }
}
