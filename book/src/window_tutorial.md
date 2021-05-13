# How to Create Windows and Display Content

## Create a Window

An application invokes the `Window::new()` function in the `window` crate to create a new window. The function would create a new `Window` object and add a weak reference of its `WindowInner` to the `WINDOW_MANAGER` instance in `window_manager`. It then returns the window to the application. Once the application terminates, the window it owns would be dropped automatically, and the weak reference in the window manager would be deleted.

## Display in a Window

An application can create a `Displayable` and invoke `Window.display()` to display it. This method is generic and works for all kinds of displayables.

After display a displayable in its framebuffer, the window would invoke its `render()` method to render the updates to the screen. A framebuffer compositor will composite a list of framebuffers and forward the result to a final framebuffer which is mapped to the screen.

## Handle Key Inputs
An application invokes `Window.handle_event()` to handle the events sent to it. For example, an active window will receive all the key input events. An application can invoke `Window.handle_event()` in a loop to handle these inputs from the keyboard.
