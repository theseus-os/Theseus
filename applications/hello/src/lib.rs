#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use alloc::{Vec, String};


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    println!("Hello, world! Args: {:?}", _args);
    println!("The development of Linux is one of the most prominent examples of free and open-source software collaboration. The underlying source code may be used, modified and distributed—commercially or non-commercially—by anyone under the terms of its respective licenses, such as the GNU General Public License.

Some of the most popular and mainstream Linux distributions[28][29][30] are Arch Linux, CentOS, Debian, Raspbian, Fedora, Gentoo Linux, Linux Mint, Mageia, openSUSE and Ubuntu, together with commercial distributions such as Red Hat Enterprise Linux and SUSE Linux Enterprise Server. Distributions include the Linux kernel, supporting utilities and libraries, many of which are provided by the GNU Project, and usually a large amount of application software to fulfil the distribution's intended use. Desktop Linux distributions include a windowing system, such as X11, Mir or a Wayland implementation, and an accompanying desktop environment such as GNOME or KDE Plasma; some distributions may also include a less resource-intensive desktop, such as LXDE or Xfce. Distributions intended to run on servers may omit all graphical environments from the standard install, and instead include other software to set up and operate a solution stack such as LAMP. Because Linux is freely redistributable, anyone may create a distribution for any intended use.");
    println!("The development of Linux is one of the most prominent examples of free and open-source software collaboration. The underlying source code may be used, modified and distributed—commercially or non-commercially—by anyone under the terms of its respective licenses, such as the GNU General Public License.

Some of the most popular and mainstream Linux distributions[28][29][30] are Arch Linux, CentOS, Debian, Raspbian, Fedora, Gentoo Linux, Linux Mint, Mageia, openSUSE and Ubuntu, together with commercial distributions such as Red Hat Enterprise Linux and SUSE Linux Enterprise Server. Distributions include the Linux kernel, supporting utilities and libraries, many of which are provided by the GNU Project, and usually a large amount of application software to fulfil the distribution's intended use. Desktop Linux distributions include a windowing system, such as X11, Mir or a Wayland implementation, and an accompanying desktop environment such as GNOME or KDE Plasma; some distributions may also include a less resource-intensive desktop, such as LXDE or Xfce. Distributions intended to run on servers may omit all graphical environments from the standard install, and instead include other software to set up and operate a solution stack such as LAMP. Because Linux is freely redistributable, anyone may create a distribution for any intended use.");

    0
}
