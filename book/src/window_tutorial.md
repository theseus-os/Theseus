# The Window Trait

The `window` crate defines a `Window` trait. It contains basic methods of operations on a window such as setting its states or clear its contents. Any structure who implements the window trait can act as a window. A window object can be owned by an application or the window manager.

# The WindowList structure

The `window_list` crates defines a `WindowList` structure. This structure consists of an active window and a list of background windows. It takes a type parameter to specify the concrete type of the `Window` objects. The structure implements basic methods to manipulate the list such as adding a new window or delete a window. Usually a window manager holds an instance of the `WindowList` structure.

The structure also implements two functions `switch_to` and `switch_to_next` to switch to a specificed window or to the next window. The order of windows is based on the last time it becomes active. The one which was active most recently is at the top of the background list. The active window would show on top of all windows and get all the key input events passed to the window manager. Once an active window is deleted, the next window in the background list will become active.

The `WindowList` structure contains a method `send_event_to_active` to send an event to the active window. The type of events are defined in the `event_type` crate. For example, `InputEvent` represents the key inputs received by the `input_event_manager`, and a window manager can invoke this method to send the key inputs to the active window.

# The Design of WindowGeneric and WindowProfile

The `window_manager` owns an instance of `WindowList` which contains all the existing windows. It invokes the methods of the object to manage these windows.

In most of the cases, both an application and the window manager wants to get access to the same window. The application needs to display in the window, and the window manager requires the information and order of windows to render them to the screen. In order to share a window among an application and the window manager, we wrap it with `Mutex`. The application owns a strong reference to the window, and the window manager holds a weak reference for its life time is longer than the window.

However, `Mutex` introduces a danger of freezing. When an application wants to get access to its window, it must lock it first, does the operation and release the window. When the window is locked, the window manager cannot do most of the operations such as switching or deleting since it needs to traverse all the windows including the locked one. 

To solve this problem, we define two objects `WindowProfile` and `WindowGeneric`. `WindowProfile` only contains the information required by the window manager and imeplements the `Window` trait. The `WindowList` object in the window managre holds a list of reference to `WindowProfile`s. An application owns a `WindowGeneric` object which wraps a reference to a `WindowProfile` structure together with other states required by the application. 

# WindowProfile

A window is an object owned by an application. Compared to mainstream operating systems, the application manipulates the states of a window rather than the window manager does it, and the window would be dropped automatically when the application terminates. The window manager holds a list of reference to the profile states of every window, which is required by it to switch among windows. Theseus would minimize the profile states held by the window manager to reduce states spill.

## Create a Window

An application invokes the `new_window()` function in the `window_manager` crate to create a new window. The crate holds a `WINDOWLIST` which maintains a list of existing windows. The function would create a new `WindowGeneric` object which contains a strong reference to a `WindowProfile` object, and add a weak reference to the profile in the `WINDOWLIST`. It then returns a strong reference of this window to the application. n iOnce the application terminates, the window it owns would be dropped automatically, and the weak reference in the `WINDOWLIST` would be deleted.

The `WindowGeneric` object represents the window. Except for the profile, it also contains a framebuffer onto which the window can display its contents, a consumer which deals with the events the window receives and a list of displayables that can display themselves in the window. The `WindowProfile` contains the states required by the window manager such as the location and the size of it. In switching to another window, the manager would render the corresponding windows to the final framebuffer according to these states.

## Add Displayables

A `Displayable` is a graph which can display itself onto a framebuffer. It usually consists of basic graphs and acts as a component of a window such as a button or a text box. An application can add any object who implements the `Displayable` trait to a window and display it. Currently we have implemeted a `TextDisplay` which is a block of text. In the future we will implement other kinds of displayables.

To add a text displayable to a window, an application creates a `TextDisplay` object, and invoke `WindowGeneric.add_displayable()` to add it as a component. The displayable is identified by a name of type `String`. 

## Display in a Window

An application can invoke `WindowGeneric.display(name)` to display a displayable by its name. This method is generic and works for other kinds of displayables. 

The application can also invoke `WindowGeneric.get_concrete_display_mut::<T>(name)` to get a displayable of a concrate type `T` and modify it. The method returns error if the window does not have a displayable of `name` or the displayable is not of type `T`.

After a displayable displays itself in a window, the application should invoke `WindowGeneric.render()` to render the updates to the screen. A framebuffer compositor would composites a list of framebuffers and outputs the result to a final framebuffer which is mapped to the screen.

## Switch among Windows

The `window_manager` crates holds a list of reference to existing `WindowProfile`s. The list consists of a single active window and a list of background windows.

## Handle Key Inputs
An application invokes `WindowGeneric.get_event()` to get the events sent to it. For example, an active window will receive all the key input events. The owner of the window can invoke `WindowGeneric.get_event()` in a loop to handle all these inputs.

## Example

```rust
use text_display::TextDisplay;
use frame_buffer::Coord;

let coordinate = Coord::new(800, 800);
let width = 300;
let height = 200;

let window = window_manager::new_window(coordinate, width, height)?
let text_display = TextDisplay::new(width, height, 0xFFFFFF, 0x000000)?
let displayable: Box<dyn displayable::Displayable> = Box::new(text_display);

let display_name = "text";
window.add_displayable(&display_name, Coord::new(0, 0), displayable)?;

let text_display_ref = window.get_concrete_display_mut::<TextDisplay>(&display_name)?;
text_display_ref.set_text("Hello World");
            
window.display(&display_name)?;
window.render()?;
```