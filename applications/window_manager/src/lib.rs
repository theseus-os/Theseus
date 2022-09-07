//! This crate acts as a manager of a list of windows. It defines a `WindowManager` structure and an instance of it. 
//!
//! A window manager holds a set of `WindowInner` objects, including an active window, a list of shown windows and a list of hidden windows. The hidden windows are totally overlapped by others.
//!
//! A window manager owns a bottom framebuffer and a top framebuffer. The bottom is the background of the desktop and the top framebuffer contains a floating window border and a mouse arrow. 
//! A window manager also contains a final framebuffer which is mapped to the screen. In refreshing an area, the manager will render all the framebuffers to the final one in order: bottom -> hide list -> showlist -> active -> top.
//!
//! The window manager provides methods to update within some bounding boxes rather than the whole screen for better performance.

#![no_std]

extern crate spin;
#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate mpmc;
extern crate event_types;
extern crate compositor;
extern crate framebuffer;
extern crate framebuffer_compositor;
extern crate framebuffer_drawer;
extern crate keycodes_ascii;
extern crate mod_mgmt;
extern crate mouse_data;
extern crate path;
extern crate scheduler; 
extern crate spawn;
extern crate window_events;
extern crate shapes;
extern crate color;
extern crate memory;

use alloc::boxed::Box;
use core::convert::TryInto;
use alloc::collections::VecDeque;
use alloc::collections::BTreeSet;
use alloc::string::ToString;
use alloc::sync::{Arc, Weak};
use alloc::vec::{Vec};
use compositor::{Compositor, FramebufferUpdates, CompositableRegion};
use mpmc::Queue;
use event_types::{Event, MousePositionEvent};
use framebuffer::{Framebuffer, AlphaPixel};
use color::{Color};
use shapes::{Coord, Rectangle};
use framebuffer_compositor::{FRAME_COMPOSITOR};
use keycodes_ascii::{KeyAction, KeyEvent, Keycode};
use mouse_data::MouseEvent;
use path::Path;
use spin::{Mutex, Once, Lazy};
use window_events::{WILI,WindowToWmEvent,WmToWindowEvent};

/// The instance of the default window manager
//pub static WINDOW_MANAGER: Once<Mutex<WindowManager>> = Once::new();

/// The width and height size of mouse in number of pixels.
const MOUSE_POINTER_SIZE_Y: usize = 18;
const MOUSE_POINTER_SIZE_X: usize = 11;
/// The mouse pointer image defined as a 2-D pixel array.
static MOUSE_POINTER_IMAGE: [[Color; MOUSE_POINTER_SIZE_Y]; MOUSE_POINTER_SIZE_X] = {
    const T: Color = color::TRANSPARENT;
    const C: Color = color::BLACK; // Cursor
    const B: Color = color::WHITE; // Border
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

struct WinRef {
    framebuffer: Option<Arc<Mutex<Framebuffer<AlphaPixel>>>>,
    tow: Queue<WmToWindowEvent>,
    fromw: Queue<WindowToWmEvent>,
    coord: Coord,
    size: (usize, usize),
}

impl WinRef{
    fn resize(&mut self, r:Rectangle) -> Result<(), &'static str> {
        self.coord=r.top_left;
        self.size.0=r.bottom_right.x as usize;
        self.size.1=r.bottom_right.y as usize;
        debug!("resized to {:?} {:?}", self.coord, self.size);
        self.tow.push(WmToWindowEvent::TellSize(self.size)).map_err(|_| "Can't push to window")
    }

    fn set_position(&mut self, c:Coord){
        self.coord=c;
    }

    fn get_position(&self) -> Coord{
        self.coord
    }

    fn get_size(&self) -> (usize, usize){
        self.size
    }

    /// Returns `true` if the given `coordinate` (relative to the top-left corner of this window)
    /// is within the bounds of this window.
    pub fn contains(&self, coordinate: Coord) -> bool {
        if let Some(ref f) = self.framebuffer{
            f.lock().contains(coordinate)
        } else {false}
    }
}

/// Window manager structure which maintains a list of windows and a mouse.
pub struct WindowManager {
    /// those window currently not shown on screen
    hide_list: VecDeque<WinRef>,
    /// those window shown on screen that may overlapping each other
    show_list: VecDeque<WinRef>,
    /// the only active window, receiving all keyboard events (except for those remained for WM)
    active: Option<WinRef>, // this one is not in show_list
    /// current mouse position
    mouse: Coord,

    keyspressed: BTreeSet<Keycode>,

    tmpfb: Framebuffer<AlphaPixel>,
    /// The bottom framebuffer typically contains the background/wallpaper image, 
    /// which is displayed by default when no other windows exist on top of it.
    bottom_fb: Framebuffer<AlphaPixel>,
    /// The top framebuffer is used for overlaying visual elements atop the rest of the windows, 
    /// e.g., the mouse pointer, the border of a window being dragged/moved, etc. 
    top_fb: Framebuffer<AlphaPixel>,
    /// The final framebuffer which is mapped to the screen (the actual display device).
    final_fb: &mut Framebuffer<AlphaPixel>,
}

impl WindowManager {
fn new_window(&mut self, queues: (Queue<WindowToWmEvent>, Queue<WmToWindowEvent>)){
                        let size = self.get_screen_size();
                        if let Some(ref mut a) = self.active{
                            self.show_list.push_front(core::mem::replace(a, WinRef {
                                fromw: queues.0,
                                tow: queues.1,
                                framebuffer: None,
                                coord: Coord{x: 0, y: 0},
                                size,
                            }));
                        } else {
                            self.active = Some(WinRef {
                                fromw: queues.0,
                                tow: queues.1,
                                framebuffer: None,
                                coord: Coord{x: 0, y: 0},
                                size,
                            });
                        }
}

    /// handle keyboard event, push it to the active window if one exists
fn keyboard_handle_application(&mut self, key_input: KeyEvent) -> Result<(), &'static str> {
    if key_input.action == KeyAction::Pressed{
        self.keyspressed.insert(key_input.keycode);
    } else{
        self.keyspressed.remove(&key_input.keycode);
    }
    // First, we handle keyboard shortcuts understood by the window manager.
    // "Super + Arrow" will resize and move windows to the specified half of the screen (left, right, top, or bottom)
    if key_input.modifiers.is_super_key()  { //&& key_input.action == KeyAction::Pressed
        if self.keyspressed.contains(&Keycode::Left) || self.keyspressed.contains(&Keycode::Up) || self.keyspressed.contains(&Keycode::Right) || self.keyspressed.contains(&Keycode::Down){
            let screen_dimensions = self.get_screen_size();
            let (width, height) = (screen_dimensions.0 as isize, screen_dimensions.1 as isize);
            let new_position = Rectangle {
                top_left: Coord {
                    x: if self.keyspressed.contains(&Keycode::Left) || !self.keyspressed.contains(&Keycode::Right) {0}
                        else {width/2},
                    y: if self.keyspressed.contains(&Keycode::Up) || !self.keyspressed.contains(&Keycode::Down) {0}
                        else {height/2},
                },
                bottom_right: Coord {
                    x: if self.keyspressed.contains(&Keycode::Right) || !self.keyspressed.contains(&Keycode::Left) {width}
                        else {width/2},
                    y: if self.keyspressed.contains(&Keycode::Down) || !self.keyspressed.contains(&Keycode::Up) {height}
                        else {height/2},
                },
            };

                let position = new_position;
                if let Some(ref mut active_window) = self.active {
                    let aw_pos = active_window.get_position();
                    let aw_size = active_window.get_size();
                    /*if aw_pos == position.top_left{
                        debug!("same pos");
                    }
                    if aw_size.0 == position.bottom_right.x.try_into().unwrap(){
                        debug!("same x");
                    } else {
                        debug!("diff x: {} and {}", aw_size.0, position.bottom_right.x);
                    }
                    if aw_size.1 == position.bottom_right.y.try_into().unwrap(){
                        debug!("same y");
                    }*/
                    let same_place = (aw_pos == position.top_left && aw_size.0 == (position.bottom_right.x - aw_pos.x).try_into().unwrap() && aw_size.1 == (position.bottom_right.y - aw_pos.y).try_into().unwrap());
                    if !same_place {
                        debug!("window_manager: resizing active window to {:?}", new_position);
                        active_window.resize(position)?;
                    }
                }
        }
        if self.keyspressed.contains(&Keycode::Enter){
            return spawnshell(self.get_child_support());
        }
        return Ok(());
    }

    // Any keyboard event unhandled above should be passed to the active window.
    if let Err(_e) = self.pass_keyboard_event_to_window(key_input) {
        warn!("window_manager: failed to pass keyboard event to active window. Error: {:?}", _e);
        // If no window is currently active, then something might be potentially wrong,
        // but we can likely recover in the future when another window becomes active.
        // Thus, we don't need to return a hard error here.
    }
    Ok(())
}

    /// Sets one window as active, push last active (if exists) to top of show_list. if `refresh` is `true`, will then refresh the window's area.
    /// Returns whether this window is the first active window in the manager.
    /// 
    /// TODO FIXME: (kevinaboos) remove this dumb notion of "first active". This is a bad hack. 




    /// delete a window and refresh its region
    pub fn delete_active(&mut self) -> Result<(), &'static str> {
        if let Some(ref current_active) = self.active {
                let (top_left, bottom_right) = {
                    let top_left = current_active.get_position();
                    let (width, height) = current_active.get_size();
                    let bottom_right = top_left + (width as isize, height as isize);
                    (top_left, bottom_right)
                };
                let area = Some(
                    Rectangle {
                        top_left: top_left,
                        bottom_right: bottom_right
                    }
                );
                if let Some(window) = self.show_list.remove(0) {
                    self.active = Some(window);
                } else if let Some(window) = self.hide_list.remove(0) {
                    self.active = Some(window);
                } else {
                    self.active = None; // delete reference
                }
                return Ok(());
        }

        Err("no active window")
    }

    /// Refresh the region of `bounding_box` in the top framebuffer
    pub fn refresh_top<B: CompositableRegion + Clone>(
        &mut self, 
        bounding_box: impl IntoIterator<Item = B> + Clone
    ) -> Result<(), &'static str> {
        let top_buffer = FramebufferUpdates {
            src_framebuffer: &self.top_fb,
            coordinate_in_dest_framebuffer: Coord::new(0, 0),
        }; 

        FRAME_COMPOSITOR.lock().composite(Some(top_buffer), &mut self.tmpfb, &mut self.final_fb, bounding_box)
    }

    
    /// Passes the given keyboard event to the currently active window.
    fn pass_keyboard_event_to_window(&self, key_event: KeyEvent) -> Result<(), &'static str> {
        if let Some(ref current_active_win) = self.active {
            debug!("sent input");
            current_active_win.tow.push(WmToWindowEvent::KeyboardEvent(key_event))
                .map_err(|_e| "Failed to enqueue the keyboard event; window event queue was full.")?;
        } else {
            return Err("cannot find active window to send key_event");
        }
        Ok(())
    }

    /// Passes the given mouse event to the window that the mouse is currently over. 
    /// 
    /// If the mouse is not over any window, an error is returned; 
    /// however, this error is quite common and expected when the mouse is not positioned within a window,
    /// and is not a true failure. 
    fn pass_mouse_event_to_window(&self, mouse_event: MouseEvent) -> Result<(), &'static str> {
        let coordinate = { &self.mouse };

        // TODO: FIXME:  improve this logic to just send the mouse event to the top-most window in the entire WM list,
        //               not just necessarily the active one. (For example, scroll wheel events can be sent to non-active windows).


        // first check the active one
        if let Some(ref current_active_win) = self.active {
                // debug!("pass to active: {}, {}", event.x, event.y);
                current_active_win.tow.push(WmToWindowEvent::MouseEvent(mouse_event))
                    .map_err(|_e| "Failed to enqueue the mouse event; window event queue was full.")?;
                return Ok(());
        }

        // TODO FIXME: (kevinaboos): the logic below here is actually incorrect -- it could send mouse events to an invisible window below others.

        // then check show_list
        for now_inner in &self.show_list {
                let current_coordinate = now_inner.get_position();
                if now_inner.contains(*coordinate) {
                    now_inner.tow.push(WmToWindowEvent::MouseEvent(mouse_event))
                        .map_err(|_e| "Failed to enqueue the mouse event; window event queue was full.")?;
                    return Ok(());
                }
        }

        Err("the mouse position does not fall within the bounds of any window")
    }

    /// take active window's base position and current mouse, move the window with delta
    pub fn move_active_window(&mut self) -> Result<(), &'static str> {
        if let Some(ref mut current_active_win) = self.active {
            let (old_top_left, old_bottom_right, new_top_left, new_bottom_right) = {
                        let old_top_left = current_active_win.get_position();
                        let new_top_left = old_top_left + (1, 1);
                        let (width, height) = current_active_win.get_size();
                        let old_bottom_right = old_top_left + (width as isize, height as isize);
                        let new_bottom_right = new_top_left + (width as isize, height as isize);
                        current_active_win.set_position(new_top_left);

                        (old_top_left, old_bottom_right, new_top_left, new_bottom_right)        
            };
            self.refresh_mouse()?;
        } else {
            return Err("cannot find active window to move");
        }
        Ok(())
    }

    /// Refresh the mouse display
    pub fn refresh_mouse(&mut self) -> Result<(), &'static str> {
        let bounding_box = Some(Rectangle {
            top_left: self.mouse,
            bottom_right: self.mouse + (MOUSE_POINTER_SIZE_X as isize, MOUSE_POINTER_SIZE_Y as isize)
        });
        
        self.refresh_top(bounding_box.into_iter())
    }

    /// Move mouse. `relative` indicates the new position relative to current position.
    fn move_mouse(&mut self, relative: Coord) -> Result<(), &'static str> {
        let old = self.mouse;
        let mut new = old + relative;
        
        let (screen_width, screen_height) = self.get_screen_size();
        if new.x < 0 {
            new.x = 0;
        }
        if new.y < 0 {
            new.y = 0;
        }

        // keep mouse pointer border in the screen when it is at the right or bottom side.
        const MOUSE_POINTER_BORDER: isize = 3;
        new.x = core::cmp::min(new.x, screen_width as isize - MOUSE_POINTER_BORDER);
        new.y = core::cmp::min(new.y, screen_height as isize - MOUSE_POINTER_BORDER);
            
        self.move_mouse_to(new)
    }
    
    // Move mouse to absolute position `new`
    fn move_mouse_to(&mut self, new: Coord) -> Result<(), &'static str> {
        // clear old mouse
        for y in self.mouse.y..self.mouse.y + MOUSE_POINTER_SIZE_Y as isize {
            for x in
                self.mouse.x..self.mouse.x + MOUSE_POINTER_SIZE_X as isize {
                let coordinate = Coord::new(x, y);
                self.top_fb.overwrite_pixel(coordinate, color::TRANSPARENT.into());
            }
        }
        let bounding_box = Some(Rectangle {
            top_left: self.mouse,
            bottom_right: self.mouse + (MOUSE_POINTER_SIZE_X as isize, MOUSE_POINTER_SIZE_Y as isize)
        });

        // draw new mouse
        self.mouse = new;
        for y in new.y..new.y + MOUSE_POINTER_SIZE_Y as isize {
            for x in new.x..new.x + MOUSE_POINTER_SIZE_X as isize {
                let coordinate = Coord::new(x, y);
                let pixel = MOUSE_POINTER_IMAGE[(x - new.x) as usize][(y - new.y) as usize].into();
                self.top_fb.overwrite_pixel(coordinate, pixel);
            }
        }
        self.refresh_mouse()?;

        Ok(())
    }

    /// Returns the `(width, height)` in pixels of the screen itself (the final framebuffer).
    pub fn get_screen_size(&self) -> (usize, usize) {
        self.final_fb.get_size()
    }

/// handle mouse event, push it to related window or anyone asked for it
fn cursor_handle_application(&mut self, mouse_event: MouseEvent) -> Result<(), &'static str> {
    if let Err(_) = self.pass_mouse_event_to_window(mouse_event) {
        // the mouse event should be passed to the window that satisfies:
        // 1. the mouse position is currently in the window area
        // 2. the window is the top one (active window or show_list windows) under the mouse pointer
        // if no window is found in this position, that is system background area. Add logic to handle those events later
    }
    Ok(())
}

}

/// Initialize the window manager. It returns (keyboard_producer, mouse_producer) for the I/O devices.
pub fn init(final_framebuffer: &mut Framebuffer<AlphaPixel>, key_consumer: Queue<KeyEvent>, mouse_consumer: Queue<MouseEvent>) -> Result<(), &'static str> {
    let (width, height) = final_framebuffer.get_size();

    let mut bottom_framebuffer = Framebuffer::new(width, height, None)?;
    let mut top_framebuffer = Framebuffer::new(width, height, None)?;
    let (screen_width, screen_height) = bottom_framebuffer.get_size();
    bottom_framebuffer.fill(color::LIGHT_GRAY.into());
    top_framebuffer.fill(color::TRANSPARENT.into()); 

    // the mouse starts in the center of the screen.
    let center = Coord {
        x: screen_width as isize / 2,
        y: screen_height as isize / 2,
    }; 

    // initialize static window manager
    let window_manager = WindowManager {
        hide_list: VecDeque::new(),
        show_list: VecDeque::new(),
        active: None,
        mouse: center,
        keyspressed: BTreeSet::new(),
        tmpfb: Framebuffer::new(width, height, None)?,
        bottom_fb: bottom_framebuffer,
        top_fb: top_framebuffer,
        final_fb: final_framebuffer,
    };
    //let _wm = WINDOW_MANAGER.call_once(|| Mutex::new(window_manager));

    // wm.refresh_bottom_windows(None, false)?;

    spawn::new_task_builder(window_manager_loop, (window_manager, key_consumer, mouse_consumer))
        .name("window_manager_loop".to_string())
        .spawn()?;

    Ok(())
}


/// handles all keyboard and mouse movement in this window manager
fn window_manager_loop(
    (mut wm, key_consumer, mouse_consumer): (WindowManager, Queue<Event>, Queue<Event>),
) -> Result<(), &'static str> {
    let screen_size = wm.get_screen_size();
    loop {
        if let Some(Event::KeyboardEvent(ref input_event)) = key_consumer.pop(){
            let key_input = input_event.key_event;
            wm.keyboard_handle_application(key_input)?;
        } else if let Some(Event::MouseMovementEvent(ref mouse_event)) = mouse_consumer.pop(){
                    // mouse::mouse_to_print(&mouse_event);
                    let mouse_displacement = &mouse_event.displacement;
                    let mut x = (mouse_displacement.x as i8) as isize;
                    let mut y = (mouse_displacement.y as i8) as isize;
                    // need to combine mouse events if there pending a lot
                    loop {
                        let next_event = match mouse_consumer.pop() {
                            Some(ev) => ev,
                            _ => {
                                break;
                            }
                        };
                        match next_event {
                            Event::MouseMovementEvent(ref next_mouse_event) => {
                                if next_mouse_event.mousemove.scrolling_up
                                    == mouse_event.mousemove.scrolling_up
                                    && next_mouse_event.mousemove.scrolling_down
                                        == mouse_event.mousemove.scrolling_down
                                    && next_mouse_event.buttonact.left_button_hold
                                        == mouse_event.buttonact.left_button_hold
                                    && next_mouse_event.buttonact.right_button_hold
                                        == mouse_event.buttonact.right_button_hold
                                    && next_mouse_event.buttonact.fourth_button_hold
                                        == mouse_event.buttonact.fourth_button_hold
                                    && next_mouse_event.buttonact.fifth_button_hold
                                        == mouse_event.buttonact.fifth_button_hold
                                {
                                    x += (next_mouse_event.displacement.x as i8) as isize;
                                    y += (next_mouse_event.displacement.y as i8) as isize;
                                }
                            }
                            _ => {
                                break;
                            }
                        }
                        // next_event.mark_completed();
                    }
                    if x != 0 || y != 0 {
                        wm.move_mouse(
                            Coord::new(x as isize, -(y as isize))
                        )?;
                    }
                    wm.cursor_handle_application(*mouse_event)?; // tell the event to application, or moving window
        } else if let Some(queues) = WILI.lock().pop(){
                        wm.new_window(queues);
        } else if let Some(ref mut window) = wm.active{
            match window.fromw.pop() {
                     Some(WindowToWmEvent::Render(wframebuffer, bounding_box)) => {
                            /// Refresh the part in `bounding_box` of the active window. `bounding_box` is a region relative to the top-left of the screen. Refresh the whole screen if the bounding box is None.
                            let buffer_update = FramebufferUpdates {
                                src_framebuffer: &wframebuffer.lock(),
                                coordinate_in_dest_framebuffer: window.get_position(),
                            };
                            FRAME_COMPOSITOR.lock().composite(Some(buffer_update), &mut wm.tmpfb, &mut wm.final_fb, bounding_box)?;
                    }
                    Some(AskSize) => {_=window.tow.push(WmToWindowEvent::TellSize((screen_size)));}
                    None => {scheduler::schedule();}
            }
        } else {scheduler::schedule();};
    }
}
       // Because this task (the window manager loop) runs in a kernel-only namespace,
        // we have to create a new application namespace in order to be able to actually spawn a shell.
fn spawnshell((shell_framebuffer, key_consumer, mouse_consumer): (&mut Framebuffer<AlphaPixel>, Queue<KeyEvent>, Queue<MouseEvent>))-> Result<(), &'static str> {
            let new_app_namespace = mod_mgmt::create_application_namespace(None)?;
            let shell_objfile = new_app_namespace.dir().get_file_starting_with("shell-")
                .ok_or("Couldn't find shell application file to run upon Ctrl+Alt+T")?;
            let path = Path::new(shell_objfile.lock().get_absolute_path());
            spawn::new_application_task_builder(path, Some(new_app_namespace))?
                .name(format!("shell"))
                .spawn()?;

            debug!("window_manager: spawned new shell app in new app namespace.");

            return Ok(());

}
