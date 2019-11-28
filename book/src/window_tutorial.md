# Tutorial of the display subsystem

## Create a Window

An application invokes the `Window::new()` function in the `window` crate to create a new window. The function would create a new `Window` object and add a weak reference of its `WindowProfile` to the `WINDOW_MANAGER` instance held by the manager. It then returns the window to the application. Once the application terminates, the window it owns would be dropped automatically, and the weak reference in the window manager would be deleted.

## Add Displayables

To add a text displayable to a window, an application creates a `TextDisplay` object and invokes `Window.add_displayable()` to add it as a component. The displayable is identified by a name of type `String`. 

In the future, we will define other kinds of displayables which implement the `Displayable` trait.

## Display in a Window

An application can invoke `Window.display(name)` to display a displayable by its name. This method is generic and works for all kinds of displayables. 

The application can also invoke `Window.get_concrete_display_mut::<T>(name)` to get access to a displayable of a concrete type `T`. The method returns error if the window does not have a displayable of `name`, or the displayable is not of type `T`.

After a displayable displays itself in a window, the window would invoke its `render()` method to render the updates to the screen. A framebuffer compositor will composite a list of framebuffers and forward the result to a final framebuffer which is mapped to the screen.

## Handle Key Inputs
An application invokes `Window.handle_event()` to handle the events sent to it. For example, an active window will receive all the key input events. An application can invoke `Window.handle_event()` in a loop to handle these inputs from the keyboard.

## Example
The application `new_window` is an example of how to create a new half-transparent window and handle the event to close the window.

To test the application, run `new_ window x y width height` in the terminal in which (x, y) is the top-left point of the window relative to the top-left of the screen and (width, height) is the size of the new window.