
use super::{get_window_obj};
use alloc::vec::Vec;
use spin::{Once, Mutex};
use frame_buffer;
use frame_buffer_3d;
use frame_buffer_text;

use acpi::ACPI_TABLE;  

pub fn test_cursor(_: Option<u64>) {
    let mut x=20;
    let mut y=20;
    let width=200;
    let height=150;
    let color = 0xe4cf8e;
    let rs = get_window_obj(x,y,width + 2,height + 2);
    if rs.is_err() {
        trace!("{}", rs.err().unwrap());
        return;
    }

    let window_mutex = rs.ok().unwrap().upgrade().unwrap();
    let window = window_mutex.lock();

           
    use keycodes_ascii::Keycode;
    (*window).draw_square(x, y, 20, 20, 0xe4cf8e);

    let mut direction = Keycode::Right;
    unsafe { 
        loop{
            let window = *window;
            let keycode = window.get_key_code();
           
            if(keycode.is_some()){
                direction = keycode.unwrap();
            }
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
}

pub fn test_draw(_: Option<u64>) {
    
    let width=300;
    let height=200;
    let mut x=width/2;
    let mut y=height/2;
    let color = 0xa71368;
    let rs = get_window_obj(300,200,width + 2,height + 2);
    if rs.is_err() {
        trace!("{}", rs.err().unwrap());
        return;
    }

    let window_mutex = rs.ok().unwrap().upgrade().unwrap();
    let window = window_mutex.lock();

    use keycodes_ascii::Keycode;
    (*window).draw_pixel(x, y, color);

    let mut direction = Keycode::Right;
    loop{
        let window = *window;
        let keycode = window.get_key_code();
        
        if(keycode.is_some()){
            direction = keycode.unwrap();
        
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

}

/*pub fn test_performance(_: Option<u64>) {
    
for lop in 1..20 {
    let hpet = ACPI_TABLE.hpet.read();
    let starting_time = (*hpet).as_ref().unwrap().get_counter();
    for i in 1..100 {
        frame_buffer::draw_square(20, 30, 200, 300, 0x354615);
        frame_buffer::draw_square(20, 30, 200, 300, 0xFFFFFF);

    }
    let end_time = (*hpet).as_ref().unwrap().get_counter();
    trace!("Time: {}", end_time - starting_time);
} 
}*/


pub fn test_performance(_: Option<u64>) {
    
    for lop in 0..50 {
        let hpet = ACPI_TABLE.hpet.read();
        let starting_time = (*hpet).as_ref().unwrap().get_counter();
        let mut color = 0x342513;
        for i in 0..100 {
            for x in 20..300{
                for y in 30..300{
                            frame_buffer::draw_pixel(x, y, color);
                }
            }
            color = color + 20;
        }
        let end_time = (*hpet).as_ref().unwrap().get_counter();
        trace!("Time: {}", end_time - starting_time);
    } 

    for lop in 0..50 {
        let hpet = ACPI_TABLE.hpet.read();
        let starting_time = (*hpet).as_ref().unwrap().get_counter();
        let mut color = 0x342513;
        for i in 1..100 {
            for x in 20..200{
                for y in 30..300{
                            frame_buffer_3d::draw_pixel(x, y, color);
                }
            }
            color = color + 20;
        }
        let end_time = (*hpet).as_ref().unwrap().get_counter();
        trace!("Time: {}", end_time - starting_time);
    } 

}

pub fn test_text(_: Option<u64>) {
    
    frame_buffer_text::print('a', 4, 7, 0xDF3546);
    frame_buffer_text::print(' ', 4, 8, 0xDF3546);
    frame_buffer_text::print('s', 4, 9, 0xDF3546);
    frame_buffer_text::print('e', 4, 10, 0xDF3546);
    frame_buffer_text::print('c', 4, 11, 0xDF3546);
    frame_buffer_text::print('r', 4, 12, 0xDF3546);
    frame_buffer_text::print('e', 4, 13, 0xDF3546);
    frame_buffer_text::print('t', 4, 14, 0xDF3546);
    frame_buffer_text::print(' ', 4, 15, 0xDF3546);
    frame_buffer_text::print('m', 4, 16, 0xDF3546);
    frame_buffer_text::print('a', 4, 17, 0xDF3546);
    frame_buffer_text::print('k', 4, 18, 0xDF3546);
    frame_buffer_text::print('e', 4, 19, 0xDF3546);
    frame_buffer_text::print('s', 4, 20, 0xDF3546);
    frame_buffer_text::print(' ', 4, 21, 0xDF3546);
    frame_buffer_text::print('a', 4, 22, 0xDF3546);
    frame_buffer_text::print(' ', 4, 23, 0xDF3546);
    frame_buffer_text::print('w', 4, 24, 0xDF3546);
    frame_buffer_text::print('o', 4, 25, 0xDF3546);
    frame_buffer_text::print('m', 4, 26, 0xDF3546);
    frame_buffer_text::print('a', 4, 27, 0xDF3546);
    frame_buffer_text::print('n', 4, 28, 0xDF3546);
    frame_buffer_text::print(' ', 4, 29, 0xDF3546);
    frame_buffer_text::print('w', 4, 30, 0xDF3546);
    frame_buffer_text::print('o', 4, 31, 0xDF3546);
    frame_buffer_text::print('m', 4, 32, 0xDF3546);
    frame_buffer_text::print('a', 4, 33, 0xDF3546);
    frame_buffer_text::print('n', 4, 34, 0xDF3546);

    

}


