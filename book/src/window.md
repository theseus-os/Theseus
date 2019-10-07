# How to Create a Window

A window is an object owned by an application. Compared to mainstream operating systems, the application owns the window and its states rather than the window manager, and the window would be dropped automatically when the application terminates. The window manager holds a list of reference to the profile states of every window, which is required for it to switch among windows. Theseus would minimize the profile states held by the window manager to reduce states spill.

## Create a Window

An application invokes the `new_window()` function in the `window_manager` crate to create a new window. The crate holds a `WINDOWLIST` which maintains a list of existing windows. The function would create a new `WindowGeneric` object which contains a strong reference to a `WindowProfile` object, and add a weak reference to the profile in the `WINDOWLIST`. Then it returns a strong reference of this window to the application. Once the application terminates, the window it owns would be dropped automatically, and the weak reference in the `WINDOWLIST` would be deleted.

The `WindowGeneric` object represents the window. Except for the profile, it also contains a framebuffer onto which the window can display its contents, a consumer which deals with the events the window receives and a list of displayables that can display themselves in the window. The `WindowProfile` contains the states required by the window manager such as the location and the size of it. In switching to another window, the manager would render the corresponding windows to the final framebuffer according to these states.

## Add Displayables

A `Displayable` is a graph which can display itself onto a framebuffer. It usually consists of basic graphs and acts as a component of a window such as a button or a text block. An application can add any object who implements the `Displayable` trait to a window and display it. Currently we have implemeted a `TextDisplay` which is a block of text. In the future we will implement other kinds of displayables.

To add a text displayable to a window, an application creates a `TextDisplay` object, and invoke `WindowGeneric.add_displayable()` to add it as a component. The displayable is identfied by a string. 

## Display in a window

The application can use the name to get the text displayable and set its content, or invoke `WindowGeneric.display(name)` to display it. These methods are generic and in the future they will work for other kinds of displayables.

After a displayable displays itself in a window, the application should invoke `WindowGeneric.render()` to render the updates to the screen. A framebuffer compositor would composites a list of framebuffers and outputs the result to a final framebuffer which is mapped to the screen.


```rust
#[no_mangle]
pub fn main(args: Vec<String>) -> isize { ... }
```
