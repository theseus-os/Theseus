
//! Guide for displaying on the screen
//!
//! # Window Manager
//! The window manager can create, delete and switch between windows.
//! Every window has its own framebuffer for itself to display on the screen.
//! An application can create and own a window. Since the ownership of a window belongs to its creator, others cannot get access to the content of a window without the premission of its owner.
//! The window manager owns a list of references to exisiting windows. It is responsible for displaying the border of windows but cannot get access to their contents.
//! 
//! # Display in windows
//! An application uses the `new_window` function in the `window_manager` to get a `WindowObject`. It can copy or delete this object.
//!
//! If the application wants to display something in the window object, it needs a displayable. We have implemented `Displayable` for `TextDisplay` and we'll have more in the future.
//! Every displayable implements a `display` method. It can use this method to display in a framebuffer.
//!
//! After creating a window, the application uses `TextDisplay::new()` to create a text displayable. It then adds the displayable to the window object with a name and specifies the location of the displayable.
//! In order to display a string, the application invokes a `display_string` method of the window and passes the name of the displayable. The window will pick the displayable and invokes its `display` method to display the string.
//! The displayable does not keep any information about the string so that no state will last. 
//!
//! * Create a new window
//!
//! `fn new_window(x: usize, y: usize, width: usize, height: usize) -> Result<WindowObj, &'static str>`
//! 
//! This function returns a new window object. The location of the window is at `(x,y)` of the screen and its size is `(width, height)`.
//! 
//! A window object owns a framebuffer, an event consumer, an inner object and a list of components.
//!
//! The `framebuffer` is of the same size as the window. It is mapped to some random allocated pages and can be composed to the final framebuffer by the framebuffer compositor.
//! 
//! The `event consumer` consumes input events. Only the consumer of current active window works.
//! 
//! The `inner` object specifies the location of the window. The window manager crate owns a list of inner objects belonging to existing windows. The manager is able to switch between these inner objects and active or inactive them.
//!
//! A window owns a list of components and uses them to display on the screen.
//!
//! * Add components to a window
//!
//! Evenry components of a window is represented by a `Displayable`.
//! An application can create a `Displayable` and add it to a window. A `Displayable` implements a `display()` function.
//! We have implemented `Displayable` for the `TextDisplay` structure. A `TextDisplay` can display a string in a framebuffer.
//! 
//! An application uses `TextDisplay::new(width:usize, height:usize)` to get a text displayable. `(width, height)` specifices the text box in which the string will display.
//!
//! An application uses `WindowObj.add_displayable(&mut self, key: &str, x: usize, y: usize, displayable: Displayable,) -> Result<(), &'static str>`
//! to add a displayable to its components list.
//! Every displayable owned by a window is of a unique `key`. It will display at location `(x, y)` of a window.
//!
//! * Display string
//!
//! An application uses `WindowObj.display_string(display_name: &str, slice: &str, font_color: u32, bg_color: u32,)` to display a string in its window
//!
//! `display_name` specifies the `key` of a displayable in the components list. `slice` is the string to display. `font_color` is the color of the text, and `bg_color` is the background color of the text box.
//!
//! The `display_string` function invokes `TextDisplay.display()` to display the string.
//!
//! In order to avoid states spill, a displayable does not keep any information about the text as states. The text is owned by the application and will not last after display.
//!
//! * Delete a window
//!
//! The content of a window will be cleaned automatically when the window object is dropped.
//! 
//! # Display on the screen
//! 
//! * framebuffer.
//! 
//! All the `display` methods of displayables are based on the `frame_buffer` crate. In Theseus,
//! every entity can own a framebuffer and write pixels in it. For a window, any displayable in its components list
//! uses the `display` method to get access to its framebuffer and invokes the basic draw functions to draw in it.
//!
//! All the basic functions are defined in `frame_buffer_drawer` and `frame_buffer_printer`.
//!
//! For example, in its `display` method, a text displayable gets the framebuffer of it owner window and invokes the `frame_buffer_printer::print_by_bytes` function.
//! The function gets a framebuffer and an argument and prints the string in it.
//! After `display` is done, the window will invoke `frame_buffer_compositor::FRAME_COMPOSITOR` to composite its framebuffer to the final framebuffer.
//!
//!
//! * compositor
//! 
//! The `frame_buffer` crate holds a static final framebuffer which is mapped to the physical framebuffer.
//!
//! `frame_buffer_compositor::FRAME_COMPOSITOR` implements a `compose` method. The method accepts a list of framebuffers and compose them together to the final framebuffer buffer.
//!
//! The `FRAME_COMPOSITOR` caches a list of updated framebuffers. If a framebuffer has not changed since last composing, it will be ignored by the `compose` method.
//!
//! # Example
//! * create a window and add a text displayable to it. print "Hello World" in the window with the text displayable
//! 
//! ```
//! let window = new_window(10, 10, 600, 600)?;
//! let text_display = TextDisplay::new(100, 100)?;
//! window.add_displayable("hello", 0, 0, text_display)?;
//! window.display_string("hello", "Hello World", 0xFFFFFF, 0x000000)?;
//! ```