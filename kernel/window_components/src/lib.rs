//! This Window Components is designated to help user to build easy-to-use GUI applications
//!
//! The `window_components` object implements the `Window` trait. It should be owned by application, see `applications/new_window` as a demo to use this library
//!
//! This library will create a window with default title bar and border. It handles the commonly used interactions like moving
//! the window or close the window. Also, it is responsible to show title bar differently when window is active. The library
//! frees applications from handling the complicated interaction with window manager, however, advanced users could learn from
//! this library about how to use window manager APIs directly.
//!
//! Currently the component only supports `textarea` which is a fixed size area to display matrix of characters. User could implement
//! other components in other crate, means this library has not a centralized control of all components, leaving flexibility to user.
//! Basically, `textarea` is initialized with the Arc of window object which is initialized by `window_components`, means,
//! `window_components` and `textarea` are in the same level, except for the priority to handle events.

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
extern crate mouse;
extern crate window;
extern crate window_manager_alpha;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::IntoIter;
use compositor::Compositor;
use core::ops::Deref;
use core::ops::DerefMut;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use displayable::Displayable;
use event_types::{Event, MousePositionEvent};
use frame_buffer::{Coord, FrameBuffer, Pixel, RectArea};
use frame_buffer_alpha::{AlphaPixel, PixelMixer, BLACK};
use frame_buffer_compositor::{FrameBufferBlocks, FRAME_COMPOSITOR, Block};
use spin::Mutex;
use window::{Window, WindowProfile};
use window_manager_alpha::{WindowProfileAlpha, WINDOW_MANAGER};

/// The title bar size, in number of pixels
const WINDOW_TITLE_BAR: usize = 15;
/// left, right, bottom border size, in number of pixels
const WINDOW_BORDER: usize = 2;
/// border radius, in number of pixels
const WINDOW_RADIUS: usize = 5;
/// border and title bar color when window is inactive
const WINDOW_BORDER_COLOR_INACTIVE: AlphaPixel = 0x00333333;
/// border and title bar color when window is active, the top part color
const WINDOW_BORDER_COLOR_ACTIVE_TOP: AlphaPixel = 0x00BBBBBB;
/// border and title bar color when window is active, the bottom part color
const WINDOW_BORDER_COLOR_ACTIVE_BOTTOM: AlphaPixel = 0x00666666;
/// window button color: red
const WINDOW_BUTTON_COLOR_CLOSE: AlphaPixel = 0x00E74C3C;
/// window button color: green
const WINDOW_BUTTON_COLOR_MINIMIZE_MAMIMIZE: AlphaPixel = 0x00239B56;
/// window button color: purple
const WINDOW_BUTTON_COLOR_HIDE: AlphaPixel = 0x007D3C98;
/// window button margin from left, in number of pixels
const WINDOW_BUTTON_BIAS_X: usize = 12;
/// the interval between buttons, in number of pixels
const WINDOW_BUTTON_BETWEEN: usize = 15;
/// the button size, in number of pixels
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

/// abstraction of a window, providing title bar which helps user moving, close, maximize or minimize window
pub struct WindowComponents {
    /// the window object that could be used to initialize components
    pub winobj: Arc<Mutex<WindowProfileAlpha>>,
    /// the width of border, init as WINDOW_BORDER. the border is still part of the window and remains flexibility for user to change border style or remove border. However, for most application a border is useful for user to identify the region.
    border_size: usize,
    /// the height of title bar in pixel, init as WINDOW_TITLE_BAR. it is render inside the window so user shouldn't use this area anymore
    title_size: usize,
    /// the background of this window, init as WINDOW_BACKGROUND
    background: AlphaPixel,
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

impl Window for WindowComponents {

    fn consumer(&mut self) -> &mut DFQueueConsumer<Event> {
        &mut self.consumer
    }

    fn framebuffer(&mut self) -> Option<&mut dyn FrameBuffer> {
        None
    }

    fn get_background(&self) -> Pixel {
        self.background
    }

    fn add_displayable(
        &mut self,
        key: &str,
        coordinate: Coord,
        displayable: Box<dyn Displayable>,
    ) -> Result<(), &'static str> {
        let key = key.to_string();
        let component = Component {
            coordinate: coordinate + self.inner_position(),
            displayable: displayable
        };
        self.components.insert(key, component);
        Ok(())
    }

    fn get_displayable_mut(
        &mut self,
        display_name: &str,
    ) -> Result<&mut Box<dyn Displayable>, &'static str> {
        Ok(&mut self.components
            .get_mut(display_name)
            .ok_or("The displayable does not exist")?
            .displayable
        )
    }

    fn get_displayable(&self, display_name: &str) -> Result<&Box<dyn Displayable>, &'static str> {
        Ok(&self.components
            .get(display_name)
            .ok_or("The displayable does not exist")?
            .displayable
        )
    }

    fn get_displayable_position(&self, key: &str) -> Result<Coord, &'static str> {
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

    fn display(&mut self, display_name: &str) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let coordinate = component.get_position();

        let area = {
            let mut window = self.winobj.lock();
            let area = component.displayable.display(
                coordinate, 
                Some(window.framebuffer_mut())
            )?;
            area
        };

        self.render(Some(area))
    }

    fn render(
        &mut self,
        mut area: Option<RectArea>,
    ) -> Result<(), &'static str> {
        let coordinate = {
            let mut window = self.winobj.lock();
            window.get_position()
        };

        let mut wm = WINDOW_MANAGER
            .try()
            .ok_or("The static window manager was not yet initialized")?
            .lock();

        let absolute_area = match area {
            Some(area) => {
                Some(area + coordinate)
            },
            None => None
        };
        wm.refresh_window(absolute_area)
    }

    fn handle_event(&mut self) -> Result<(), &'static str> {
       
        let mut call_later_do_refresh_floating_border = false;
        let mut call_later_do_move_active_window = false;
        let mut need_to_set_active = false;
        let mut need_refresh_three_button = false;

        let is_active = window_manager_alpha::is_active(&self.winobj);
        if is_active != self.last_is_active {
            self.draw_border(is_active);
            self.last_is_active = is_active;
            let bcoordinate = {
                let mut winobj = self.winobj.lock();
                let coordinate = winobj.get_position();
                let bcoordinate = coordinate;
                self.show_button(TopButton::Close, 1, &mut winobj);
                self.show_button(TopButton::MinimizeMaximize, 1, &mut winobj);
                self.show_button(TopButton::Hide, 1, &mut winobj);
                bcoordinate
            };

            /*if let Err(err) = self.refresh_border(bcoordinate) {
                error!("refresh_border failed {}", err);
            }*/
        }

        let bcoordinate = {
            let mut winobj = self.winobj.lock();
            let consumer = &winobj.consumer;
            let event = match consumer.peek() {
                Some(ev) => ev,
                _ => {
                    return Ok(());
                }
            };

            let coordinate = winobj.get_position();
            let bcoordinate = coordinate;
            match event.deref() {
                &Event::KeyboardEvent(ref input_event) => {
                    let key_input = input_event.key_event;
                    self.producer.enqueue(Event::new_keyboard_event(key_input));
                }
                &Event::MousePositionEvent(ref mouse_event) => {
                    // debug!("mouse_event: {:?}", mouse_event);
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
            bcoordinate
        };
        if need_to_set_active {
            window_manager_alpha::set_active(&self.winobj)?;
        }
        if need_refresh_three_button {
            // if border has been refreshed, no need to refresh buttons
            if let Err(err) = self.refresh_three_button(bcoordinate) {
                error!("refresh_three_button failed {}", err);
            }
        }

        if call_later_do_refresh_floating_border {
            window_manager_alpha::do_refresh_floating_border()?;
        }

        if call_later_do_move_active_window {
            if let Err(err) = window_manager_alpha::do_move_active_window() {
                error!("do_move_active_window failed {}", err);
            }
        }

        if call_later_do_refresh_floating_border || call_later_do_move_active_window {
            let wm = window_manager_alpha::WINDOW_MANAGER.try().ok_or("The window manager is not initialized")?.lock();
            wm.refresh_background(None)?;
            wm.refresh_window(None)?;
            wm.refresh_top(None)?;
        }
        
        Ok(())
    }
}

impl WindowComponents {
    /// create new WindowComponents by given position and size, return the Mutex of it for ease of sharing
    /// x, y is the distance in pixel relative to top-left of window
    pub fn new(
        coordinate: Coord,
        framebuffer: Box<dyn FrameBuffer>,
        background: u32
    ) -> Result<WindowComponents, &'static str> {
        let (width, height) = framebuffer.get_size();
        if width <= 2 * WINDOW_TITLE_BAR || height <= WINDOW_TITLE_BAR + WINDOW_BORDER {
            return Err("window too small to even draw border");
        }

        let winobj_mutex = window_manager_alpha::new_window(coordinate, framebuffer)?;

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

        let (start, end) = {
            let mut winobj = wincomps.winobj.lock();
            winobj.framebuffer.fill_color(wincomps.background);
            let coordinate = winobj.get_position();
            let start = coordinate;
            let end = start + (winobj.width as isize, winobj.height as isize);
            (start, end)
        };
        wincomps.draw_border(true); // draw window with active border
                                    // draw three buttons
        {
            let mut winobj = wincomps.winobj.lock();
            wincomps.show_button(TopButton::Close, 1, &mut winobj);
            wincomps.show_button(TopButton::MinimizeMaximize, 1, &mut winobj);
            wincomps.show_button(TopButton::Hide, 1, &mut winobj);
        }
        debug!("before refresh");
        wincomps.render(None)?;
        // window_manager_alpha::refresh_area_absolute(start, end)?;
        debug!("after refresh");

        Ok(wincomps)
    }

    pub fn init_displayable(&mut self, display_name: &str) -> Result<(), &'static str> {
        let component = self.components.get_mut(display_name).ok_or("")?;
        let coordinate = component.get_position();

        {
            let mut window = self.winobj.lock();
            component.displayable.clear(
                coordinate, 
                Some(window.framebuffer_mut())
            )?;
        }

        self.render(None)
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

    /// draw the border of this window, with argument of whether this window is active now
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

        // winobj
        //     .framebuffer
        //     .draw_rect(
        //         Coord::new(0, self.title_size as isize),
        //         Coord::new(self.border_size as isize, height as isize),
        //         border_color
        //     );
        // winobj
        //     .framebuffer
        //     .draw_rect(
        //         Coord::new(0, (height - self.border_size) as isize),
        //         Coord::new(width as isize, height as isize),
        //         border_color
        //     );
        // winobj.framebuffer.draw_rect(
        //     Coord::new((width - self.border_size) as isize, self.title_size as isize),
        //     Coord::new(width as isize, height as isize),
        //     border_color,
        // );
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
                ); // winobj.framebuffer.draw_rect(
                   //     Coord::new(0, i as isize),
                   //     Coord::new(width as isize, i as isize + 1),
                   //     WINDOW_BORDER_COLOR_ACTIVE_BOTTOM.color_mix(
                   //         WINDOW_BORDER_COLOR_ACTIVE_TOP,
                   //         (i as f32) / (self.title_size as f32),
                   //     ),
                   // );
            }
        } else {
            frame_buffer_drawer::draw_rectangle(
                winobj.framebuffer.deref_mut(),
                Coord::new(0, 0),
                width,
                self.title_size,
                border_color,
            );
            // winobj
            //     .framebuffer
            //     .draw_rect(
            //         Coord::new(0, 0),
            //         Coord::new(width as isize, self.title_size as isize),
            //         border_color
            //     );
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

    /// refresh border, telling window manager to refresh
    fn refresh_border(&self, bcoordinate: Coord) -> Result<(), &'static str> {
        let (width, height) = {
            let winobj = self.winobj.lock();
            let width = winobj.width;
            let height = winobj.height;
            (width as isize, height as isize)
        };
        let border_size = self.border_size as isize;
        let title_size = self.title_size as isize;
        window_manager_alpha::refresh_area_absolute(
            bcoordinate + (0, title_size),
            bcoordinate + (border_size, height),
        )?;
        window_manager_alpha::refresh_area_absolute(
            bcoordinate + (0, height - border_size),
            bcoordinate + (width, height),
        )?;
        window_manager_alpha::refresh_area_absolute(
            bcoordinate + (width - border_size, title_size),
            bcoordinate + (width, height),
        )?;
        window_manager_alpha::refresh_area_absolute(
            bcoordinate,
            bcoordinate + (width, title_size),
        )?;
        Ok(())
    }

    /// show three button with status. state = 0,1,2 for three different color
    fn show_button(&self, button: TopButton, state: usize, winobj: &mut WindowProfileAlpha) {
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
    fn refresh_three_button(&self, bcoordinate: Coord) -> Result<(), &'static str> {
        for i in 0..3 {
            let coordinate = bcoordinate
                + (
                    WINDOW_BUTTON_BIAS_X as isize + i * WINDOW_BUTTON_BETWEEN as isize,
                    self.title_size as isize / 2,
                );
            let r = WINDOW_RADIUS as isize;

            let profile = self.winobj.lock();
            let frame_buffer_blocks = FrameBufferBlocks {
                framebuffer: profile.framebuffer.deref(),
                coordinate: profile.coordinate,
                blocks: None,
            };
            FRAME_COMPOSITOR
                .lock()
                .composite(vec![frame_buffer_blocks].into_iter())?;

            /* window_manager_alpha::refresh_area_absolute(
                coordinate - (r, r),
                coordinate + (r + 1, r + 1),
            )?;*/
        }
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

    /// return the available inner size, excluding title bar and border
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
                } else {
                    trace!("delete window succeed");
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
    // gets the displayable
    fn get_displayable(&self) -> &Box<dyn Displayable> {
        return &(self.displayable);
    }

    // gets a mutable reference to the displayable
    fn get_displayable_mut(&mut self) -> &mut Box<dyn Displayable> {
        return &mut (self.displayable);
    }

    // gets the coordinate of the displayable relative to the top-left corner of the window
    fn get_position(&self) -> Coord {
        self.coordinate
    }

    // resizes the displayable as (width, height) at `coordinate` relative to the top-left corner of the window
    fn resize(&mut self, coordinate: Coord, width: usize, height: usize) {
        self.coordinate = coordinate;
        self.displayable.resize(width, height);
    }
}
