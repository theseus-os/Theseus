# How the window manager works

## Design

In most of the cases, both an application and the window manager want to get access to the same window. The application needs to display in the window, and the window manager requires the information and order of windows to render them to the screen. In order to share a window between an application and the window manager, we wrap a window object with `Mutex`. The application owns a strong reference to the window, while the window manager holds a weak reference since its lifetime is longer than the window.

However, `Mutex` introduces a danger of deadlocks. When an application wants to get access to its window, it must lock it first, operate on it and then release it. If an application does not release the locked window, the window manager will be blocked in most of the operations such as switching or deleting since it needs to traverse all the windows including the locked one. 

To solve this problem, we define two objects `Window` and `WindowInner`. `WindowInner` only contains the information required by the window manager. A window manager holds a list of reference to `WindowInner`s. An application owns a `Window` object which wraps a reference to its `WindowInner` object together with other states required by the application. 

## The WindowInner structure

The `window_inner` crate defines a `WindowInner` structure. It has states and methods of displaying the window on the screen. 

A `WindowInner` has a framebuffer in which it can display the content of the window. The framebuffer takes a type parameter of pixels it consists of. When the window is rendered to the screen, a compositor may composite every pixel with different principles according to the type. Currently, we have implemented a normal RGB pixel and a pixel of an alpha channel.

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

