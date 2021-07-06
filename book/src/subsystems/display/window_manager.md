# How the Window Manager works

## Design

Typically, both the application that owns/creates a window and the window manager that controls that window need to access it jointly. The application needs to display its content into the main part of the window, and the window manager needs information about the location and depth ordering of all windows to render them. 

To share a window between an application and the window manager, the application holds a strong reference (`Arc`) to the window, while the window manager holds a weak reference (`Weak`) to that same window. This allows the window manager to control an manage a window without conceptually owning it.

We use a `Mutex` to wrap each window to allow the application task and window manager task to safely access it jointly. However, `Mutex` introduces the possibility of deadlock: when an application wants to access its window, it must acquire the Mutex lock, operate on the window, and then release the lock. If the application doesn't release the lock on its window, the window manager will be forced to block until the lock is released, preventing it from performing typical operations like switching between windows, delivering events, or deleting windows.

To solve this problem, we define two structures: `Window` and `WindowInner`. `WindowInner` only contains the information required by the window manager. The window manager holds a list of references to `WindowInner` objects, while only the application owns the outer `Window` object (which itself does contain a reference to the underlying WM-owned `WindowInner` object. The `Window` struct also contains other application-relevant states that describe the window.

## The `WindowInner` structure

The `window_inner` crate defines a `WindowInner` structure. It has states and methods of displaying the window on the screen.

A `WindowInner` has a framebuffer to which it can display the content of the window. The framebuffer takes a type parameter of pixels it consists of. When the window is rendered to the screen, a compositor may composite every pixel with different principles according to the type. Currently, we have implemented a normal RGB pixel and a pixel of an alpha channel.

Both an application's window and the window manager has a reference to the same `WindowInner` object. The application can configure and draw in the framebuffer and the manager can display and composite the window with others.

This structure also has an event producer. The window manager gets events from I/O devices such as keyboards and push them to the corresponding producer.


## Window

A `Window` object represents a window and is owned by an application. It contains its profile, a title, a consumer and a list of displayables. The consumer can get events pushed to the producer in its profile by the manager.

A `Window` provides methods to display the displayables in it and render itself to the screen. The window manager is responsible for compositing it with other windows through a framebuffer compositor.

## Displayables

The `displayable` crate defines a `Displayable` trait. A `Displayable` is an item which can display itself onto a framebuffer. It usually consists of basic graphs and acts as a component of a window such as a button or a text box. Currently, we have implemented a `TextDisplay` which is a block of text. In the future, we will implement other kinds of displayables.

An application can own multiple displayables and display any type of `Displayable` in its window.

## The WindowManager

The `window_manager` crate defines a `WindowManager` structure. This structure consists of the profiles of an active window, a list of shown windows and a list of hidden windows. The hidden ones are totally overlapped by others. The structure implements basic methods to manipulate the list such as adding or deleting a window.

The `WindowManager` structure contains a bottom framebuffer which represents the background image and a final framebuffer of a floating window border and a mouse arrow. In refreshing an area, it renders the framebuffers in order background -> hidden list -> shown list -> active -> top. It provides several methods to update a rectangle area or several pixels for better performance.

The structure defines a loop for generic events, a loop for keyboard events and a loop for mouse events. Theseus will initialize them as tasks to handle inputs. The window manager structure provides methods to operate on the window list as reactions to these inputs. It can move a window when we drag it with mouse or pass other events to the active window. The owner application of the active window can handle these events.

The `window_manager` crate owns a `WINDOW_MANAGER` instance which contains all the existing windows. It invokes the methods of `WindowManager` to manage these windows.

