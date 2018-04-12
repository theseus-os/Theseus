
use super::{get_window_obj};
use alloc::vec::Vec;
use spin::{Once, Mutex};


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

