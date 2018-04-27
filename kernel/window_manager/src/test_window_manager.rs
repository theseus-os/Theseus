
use super::{get_window_obj};
use alloc::vec::Vec;
use spin::{Once, Mutex};
use frame_buffer;
use frame_buffer_3d;
use frame_buffer_text;

use acpi::ACPI_TABLE;  

pub fn test_cursor(_: Option<u64>) -> Option<&'static str> {
    let mut x=20;
    let mut y=20;
    let width=200;
    let height=150;
    let color = 0xe4cf8e;
    let rs = get_window_obj(x,y,width + 2,height + 2);
    if rs.is_err() {
        return rs.err();
    }

    let window_mutex_opt= try_opt!(rs.ok()).upgrade();
    let window_mutex = try_opt!(window_mutex_opt);
    let window = window_mutex.lock();

           
    use keycodes_ascii::Keycode;
    (*window).draw_square(x, y, 20, 20, 0xe4cf8e);

    let mut direction = Keycode::Right;

    loop{
        let window = *window;
        let keycode = window.get_key_code();
        direction = try_opt!(keycode);
        
        match direction {
            Keycode::Right => {
                if((y+20) < height) {
                    window.draw_line(x, y, x, y+20, 0x000000);
                    window.draw_line((20+x)%width, y, (20+x)%width, y+20, 0xe4cf8e);
                } else {
                    window.draw_line(x, y, x, height, 0x000000);
                    window.draw_line(x, 0, x, (y+20)%height, 0x000000);
                    window.draw_line((20+x)%width, y, (20+x)%width, height, 0xe4cf8e);
                    window.draw_line((20+x)%width, 0, (20+x)%width, (y+20)%height, 0xe4cf8e);                        
                }
                x = (x + 1)%width;
            }
            Keycode::Left => { 
                if((y+20) < height) {               
                    window.draw_line((x+19)%width, y, (x+19)%width, y+20, 0x000000);
                    x = (x + width -1)%width;
                    window.draw_line(x, y, x, y+20, 0xe4cf8e);
                } else {
                    window.draw_line((x+19)%width, y, (x+19)%width, height, 0x000000);
                    window.draw_line((x+19)%width, 0, (x+19)%width, (y+20)%height, 0x000000);
                    x = (x + width -1)%width;
                    window.draw_line(x, y, x, height, 0xe4cf8e);    
                    window.draw_line(x, 0, x, (y+20)%height, 0xe4cf8e);    
                }                            
            }
            Keycode::Up => {
                if (x+20<width)  {             
                    window.draw_line(x, (y+19)%height, x+20, (y+19)%height, 0x000000);
                    y = (y + height -1)%height;
                    window.draw_line(x, y, x+20, y, 0xe4cf8e);                            
                } else {
                    window.draw_line(x, (y+19)%height, width, (y+19)%height, 0x000000);
                    window.draw_line(0, (y+19)%height, (x+20)%width, (y+19)%height, 0x000000);
                    y = (y + height -1)%height;
                    window.draw_line(x, y, width, y, 0xe4cf8e);                            
                    window.draw_line(0, y, (x+20)%width, y, 0xe4cf8e);                            
                }
            }
            Keycode::Down => {    
                if (x+20<width) {      
                    window.draw_line(x, y, (x+20)%width, y, 0x000000);
                    window.draw_line(x, (y+20)%height, (20+x)%width, (y+20)%height, 0xe4cf8e);
                    y = (y + 1)%height;
                } else {
                    window.draw_line(x, y, width, y, 0x000000);
                    window.draw_line(0, y, (x+20)%width, y, 0x000000);
                    window.draw_line(x, (y+20)%height, width, (y+20)%height, 0xe4cf8e);
                    window.draw_line(0, (y+20)%height, (20+x)%width, (y+20)%height, 0xe4cf8e);
                    y = (y + 1)%height;
                }                         
            }
            
            _ => {}
        }
    }       
    
}

pub fn test_draw(_: Option<u64>) -> Option<&'static str>{
    
    let width=300;
    let height=200;
    let mut x=width/2;
    let mut y=height/2;
    let color = 0xa71368;
    let rs = get_window_obj(300,200,width + 2,height + 2);
    if rs.is_err() {
        return rs.err();
    }

    let window_mutex_opt= try_opt!(rs.ok()).upgrade();
    let window_mutex = try_opt!(window_mutex_opt);
    let window = window_mutex.lock();

    use keycodes_ascii::Keycode;
    (*window).draw_pixel(x, y, color);

    let mut direction = Keycode::Right;
    loop{
        let window = *window;
        let keycode = window.get_key_code();
        
        direction = try_opt!(keycode);
    
        match direction {
            Keycode::Right => {
                x = (x+1)%width;
            }
            Keycode::Left => { 
                x = (x + width -1)%width;
            }
            Keycode::Up => {
                y = (y + height -1)%height;    
            }
            Keycode::Down => {       
                    y = (y + 1)%height;                          
            }
            
            _ => {}
        }
        window.draw_pixel(x, y, color);
    
    }       

}


pub fn test_performance(_: Option<u64>) -> Option<&'static str> {
    
    for lop in 0..50 {
        let hpet = ACPI_TABLE.hpet.read();
        let starting_time = try_opt!((*hpet).as_ref()).get_counter();
        let mut color = 0x342513;
        for i in 0..100 {
            for x in 20..300{
                for y in 30..300{
                            frame_buffer::draw_pixel(x, y, color);
                }
            }
            color = color + 20;
        }
        let end_time = try_opt!((*hpet).as_ref()).get_counter();
        trace!("Time: {}", end_time - starting_time);
    } 

    for lop in 0..50 {
        let hpet = ACPI_TABLE.hpet.read();
        let starting_time = try_opt!((*hpet).as_ref()).get_counter();
        let mut color = 0x342513;
        for i in 1..100 {
            for x in 20..200{
                for y in 30..300{
                            frame_buffer_3d::draw_pixel(x, y, color);
                }
            }
            color = color + 20;
        }
        let end_time = try_opt!((*hpet).as_ref()).get_counter();
        trace!("Time: {}", end_time - starting_time);
    } 

    Some("End")

}

pub fn test_text(_: Option<u64>) -> Result<(), &'static str> {
    use frame_buffer_text::CONSOLE_FRAME_TEXT_BUFFER;
    use core::fmt::Write;
    for i in 0..15 {
        try!(CONSOLE_FRAME_TEXT_BUFFER.lock().write_str("This is the first sentence\nEnter an enter\n").map_err(|_| "error in FrameBuffer's write_str()"));
        try!(CONSOLE_FRAME_TEXT_BUFFER.lock().write_str("A secret is a secret\nZero is start\n").map_err(|_| "error in FrameBuffer's write_str()"));
        try!(CONSOLE_FRAME_TEXT_BUFFER.lock().write_str("I am Theseus\nThe end\n").map_err(|_| "error in FrameBuffer's write_str()"));
    }
    Ok(())

}


