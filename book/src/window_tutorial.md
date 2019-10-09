# Tutorial of the display subsystem

## Create a Window

An application invokes the `new_window()` function in the `window_manager` crate to create a new window. The function would create a new `WindowGeneric`, and add a weak reference of its `WindowProfile` to the `WINDOWLIST`. It then returns a strong reference of this window to the application. Once the application terminates, the window it owns would be dropped automatically, and the weak reference in the `WINDOWLIST` would be deleted.

## Add Displayables

To add a text displayable to a window, an application creates a `TextDisplay` object, and invoke `WindowGeneric.add_displayable()` to add it as a component. The displayable is identified by a name of type `String`. 

## Display in a Window

An application can invoke `WindowGeneric.display(name)` to display a displayable by its name. This method is generic and works for all kinds of displayables. 

The application can also invoke `WindowGeneric.get_concrete_display_mut::<T>(name)` to get access to a displayable of a concrate type `T`. The method returns error if the window does not have a displayable of `name`, or the displayable is not of type `T`.

After a displayable displays itself in a window, the application should invoke `WindowGeneric.render()` to render the updates to the screen. A framebuffer compositor would composites a list of framebuffers and outputs the result to a final framebuffer which is mapped to the screen.

## Handle Key Inputs
An application invokes `WindowGeneric.get_event()` to get the events sent to it. For example, an active window will receive all the key input events. An application can invoke `WindowGeneric.get_event()` in a loop to handle all these inputs.

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