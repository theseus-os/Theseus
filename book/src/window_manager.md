# How the window manager works

## Design

In most of the cases, both an application and the window manager want to get access to the same window. The application needs to display in the window, and the window manager requires the information and order of windows to render them to the screen. In order to share a window between an application and the window manager, we wrap a window object with `Mutex`. The application owns a strong reference to the window, while the window manager holds a weak reference since its lifetime is longer than the window.

However, `Mutex` introduces a danger of deadlocks. When an application wants to get access to its window, it must lock it first, operate on it and then release it. If an application does not release the locked window, the window manager will be blocked in most of the operations such as switching or deleting since it needs to traverse all the windows including the locked one. 

To solve this problem, we define two objects `Window` and `WindowView`. `WindowView` only contains the information required by the window manager and implements the `WindowView` trait. A window manager holds a list of reference to `WindowView`s. An application owns a `Window` object which wraps a reference to its `WindowView` object together with other states required by the application. 

## The WindowView Trait

The `window_profile` crate defines a `WindowView` trait. It has basic methods of operations on a window's information such as setting or getting its states. Any structure that implements the trait can act as the profile of a window. It is owned by a window and a window manager concurrently. The window can operate on the profile information while the manager can render it to the screen according to these informations.

## The WindowView structure

`WindowView` implements the `WindowView` trait. It contains the basic information of the window and an event producer. The window manager gets events from I/O devices such as keyboards and push them to the active producer.

A `WindowView` has a framebuffer in which it can display the content of the window. The framebuffer is an object which implements the `FrameBuffer` trait. When the window is rendered to the screen, a compositor may render it with different principles according to the type of its framebuffer. Currently, we have implemented a normal framebuffer and one with the alpha channel.

## Window

A `Window` object represents a window and is owned by an application. It contains its profile, a title, a consumer and a list of displayables. The consumer can get events pushed to the producer in its profile by the manager.

A `Window` provides methods to display the displayables in it and render itself to the screen. The window manager is responsible for compositing it with other windows with a framebuffer compositor. An application can create its window with different types of framebuffers. It would be rendered to the screen according to the implementation of the framebuffer.

## Displayables

The `displayable` crate defines a `Displayable` trait. A `Displayable` is an item which can display itself onto a framebuffer. It usually consists of basic graphs and acts as a component of a window such as a button or a text box. Currently, we have implemented a `TextDisplay` which is a block of text. In the future, we will implement other kinds of displayables.

An application can add any `Displayable` object to a window and display it. The `Window` structure identifies `Displayables` by their name. It implements generic methods to get access to different kinds of displayables or display them by their names.

## The WindowManager

The `window_manager` crate defines a `WindowManager` structure. This structure consists of the profiles of an active window, a list of shown windows and a list of hidden windows. The hidden ones are totally overlapped by others. The structure implements basic methods to manipulate the list such as adding or deleting a window. 

The `WindowManager` structure contains a bottom framebuffer which represents the background image and a final framebuffer of a floating window and a mouse arrow. In refreshing an area, it renders the framebuffers in order background -> hidden list -> shown list -> active -> top. It provides several methold to update a rectangle area or several pixels for better performance.

The structure defines a loop for generic events, a loop for keyboard events and a loop for mouse events. Theseus will initialize them as tasks to handle inputs. The window manager structure provides methods to operate on the windows as reactions to these inputs. It can move a window when we drag it with mouse or pass other events to the active window. The owner application of the window can handle these events.

The `window_manager` crate owns a `WINDOW_MANAGER` instance which contains all the existing windows. It invokes the methods of `WindowManager` to manage these windows.

