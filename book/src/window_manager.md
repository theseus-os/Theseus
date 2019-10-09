# How the window manager works

## The Window Trait

The `window` crate defines a `Window` trait. It has basic methods of operations on a window such as setting its states or clear its contents. Any structure who implements the trait can act as a window. A window object is usually owned by an application or the window manager.

## The WindowList structure

The `window_list` crate defines a `WindowList` structure. This structure consists of an active window and a list of background windows. It takes a type parameter to specify the concrete type of these `Window` objects. The structure implements basic methods to manipulate the list such as adding or deleting a window. 

The structure also implements two functions `switch_to` and `switch_to_next` to switch to a specificed window or to the next one. The order of windows is based on the last time thye become active. The one which was active most recently is at the top of the background list. The active window would show on top of all windows and get all the events passed to the window manager. Once an active window is deleted, the next window in the background list will become active.

The `WindowList` structure contains a method `send_event_to_active` to send an event to the active window. The type of events are defined in the `event_type` crate. For example, `InputEvent` represents the key inputs received by the `input_event_manager`, and a window manager can invoke this method to send the key inputs to the active window.

A window manager holds an instance of the `WindowList` structure. In the future, we will define the `WindowList` as a generic trait and implement different kinds of window managers.

## The Window Manager

The `window_manager` owns an instance of `WindowList` which contains all the existing windows. It invokes the methods of `WindowList` to manage these windows.

In most of the cases, both an application and the window manager want to get access to the same window. The application needs to display in the window, and the window manager requires the information and order of windows to render them to the screen. In order to share a window among an application and the window manager, we wrap a `Window` object with `Mutex`. The application owns a strong reference to the window, while the window manager holds a weak reference since its life time is longer than the window.

However, `Mutex` introduces a danger of deadlock. When an application wants to get access to its window, it must lock it first, does the operation and release it. If an application does not release the locked window, the window manager cannot do most of the operations such as switching or deleting since it needs to traverse all the windows including the locked one. 

To solve this problem, we define two objects `WindowProfile` and `WindowGeneric`. `WindowProfile` only contains the information required by the window manager and imeplements the `Window` trait. The `WindowList` object in the window manager holds a list of reference to `WindowProfile`s. An application owns a `WindowGeneric` object which wraps a reference to a `WindowProfile` structure together with other states required by the application. 

## WindowProfile

The `WindowProfile` structure contains the location, the size, the padding, the active state of a window and an event producer. Window manager uses the profile information to render all the windows to the screen. Once an event arrives, the window manager will push it into the producer of the active window so that the owner of the corresponding `WindowGeneric` object will handle it.

## WindowGeneric

The `WindowGeneric` object represents the window and is owned by an application. Except for the profile, it also contains a framebuffer onto which the window can display its contents, a consumer which deals with the events the window receives and a list of displayables that can display themselves in the window.

## Displayables

The `displayable` crate defines a `Displayable` trait. A `Displayable` is a graph which can display itself onto a framebuffer. It usually consists of basic graphs and acts as a component of a window such as a button or a text box. Currently we have implemeted a `TextDisplay` which is a block of text. In the future we will implement other kinds of displayables.

An application can add any `Displayable` object to a window and display it. Displayables are identified by their name. `WindowGeneric` contains generic methods to get access to different kinds of displayables or display them by their names.