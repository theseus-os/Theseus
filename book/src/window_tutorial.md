# Tutorial of the display subsystem

## Create a Window

An application invokes the `new_window()` function in the `window_manager_primitve` crate to create a new window. The function would create a new `WindowPrimitive` object and add a weak reference of its `WindowProfile` to the `WINDOWLIST` held by the manager. It then returns the window to the application. Once the application terminates, the window it owns would be dropped automatically, and the weak reference in the `WINDOWLIST` would be deleted.

## Add Displayables

To add a text displayable to a window, an application creates a `TextPrimitive` object and invokes `WindowPrimitive.add_displayable()` to add it as a component. The displayable is identified by a name of type `String`. 

In the future, we will define other kinds of displayables which implement the `Displayable` trait.

## Display in a Window

An application can invoke `WindowPrimitive.display(name)` to display a displayable by its name. This method is generic and works for all kinds of displayables. 

The application can also invoke `WindowPrimitive.get_concrete_display_mut::<T>(name)` to get access to a displayable of a concrete type `T`. The method returns error if the window does not have a displayable of `name`, or the displayable is not of type `T`.

After a displayable displays itself in a window, the window would invoke its `render()` method to render the updates to the screen. A framebuffer compositor will composite a list of framebuffers and forward the result to a final framebuffer which is mapped to the screen.

## Handle Key Inputs
An application invokes `WindowPrimitive.get_event()` to get the events sent to it. For example, an active window will receive all the key input events. An application can invoke `WindowPrimitive.get_event()` in a loop to handle these inputs from the keyboard.

## Example
This example shows how to create a window, add a text displayable to it and print "Hello World" in the window with the text displayable.

```rust
use text_primitive::TextPrimitive;
use frame_buffer::Coord;

let coordinate = Coord::new(800, 800);
let width = 300;
let height = 200;

let window = window_manager_primitive::new_window(coordinate, width, height)?
let text_primitive = TextPrimitive::new(width, height, 0xFFFFFF, 0x000000)?
let displayable: Box<dyn displayable::Displayable> = Box::new(text_primitive);

let display_name = "text";
window.add_displayable(&display_name, Coord::new(0, 0), displayable)?;

let text_primitive_ref = window.get_concrete_display_mut::<TextPrimitive>(&display_name)?;
text_primitive_ref.set_text("Hello World");
            
window.display(&display_name)?;
```