use super::write_to;
use super::super::FrameBuffer;

///draw a pixel
pub fn draw_pixel(framebuffer:&mut FrameBuffer, x:usize, y:usize, color:u32){    
    let index = framebuffer.get_index_fn();
    if framebuffer.check_in_range(x, y) {
        write_to(&mut framebuffer.buffer, index(x, y), color);
    }
}

///draw a line from (start_x, start_y) to (end_x, end_y) with color
pub fn draw_line(framebuffer:&mut FrameBuffer, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
    let width:i32 = end_x - start_x;
    let height:i32 = end_y - start_y;
    
    let index = framebuffer.get_index_fn();

    //compare the x distance and y distance. Increase/Decrease the longer one at every step.
    if width.abs() > height.abs() {
        let mut y;
        let mut x = start_x;

        //if the end_x is larger than start_x, increase x. Otherwise decrease it.
        let step = if width > 0 { 1 } else { -1 };

        loop {
            if x == end_x {
                break;
            }          
            y = (x - start_x) * height / width + start_y;
            if framebuffer.check_in_range(x as usize, y as usize) {
                write_to(&mut framebuffer.buffer, index(x as usize, y as usize), color);
            }
            x += step;
        }
    } else {
        let mut x;
        let mut y = start_y;
        let step = if height > 0 { 1 } else { -1 };
        loop {
            if y == end_y {
                break;
            }
            x = (y - start_y) * width / height + start_x;
            if { framebuffer.check_in_range(x as usize,y as usize) }{
                write_to(&mut framebuffer.buffer, index(x  as usize, y as usize), color);
            }
            y += step;   
        }
    }

}

//draw a rectangle at (start_x, start_y) with color
pub fn draw_rectangle(framebuffer:&mut FrameBuffer, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
    let index = framebuffer.get_index_fn();
    let (buffer_width, buffer_height) = framebuffer.get_size();

    let end_x:usize = { 
        if start_x + width < buffer_width { start_x + width } 
        else { buffer_width }
    };

    let end_y:usize = {
        if start_y + height < buffer_height { start_y + height } 
        else { buffer_height }
    };

    let mut x = start_x;
    let buffer = framebuffer.buffer();

    //Consider to use slice copy to increase the performance
    loop {
        if x == end_x {
            break;
        }
        buffer[index(x, start_y)] = color;
        buffer[index(x, end_y-1)] = color;
        x += 1;
    }

    let mut y = start_y;
    loop {
        if y == end_y {
            break;
        }
        buffer[index(start_x, y)] = color;
        buffer[index(end_x-1, y)] = color;
        y += 1;
    }
}

//fill a rectangle at (start_x, start_y) with color
pub fn fill_rectangle(framebuffer:&mut FrameBuffer, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
    let mut x = start_x;
    let mut y = start_y;
    

    let (buffer_width, buffer_height) = framebuffer.get_size();
    let index = framebuffer.get_index_fn();
    let end_x:usize = {
        if start_x + width < buffer_width { start_x + width } 
        else { buffer_width }
    };

    let end_y:usize = {
        if start_y + height < buffer_height { start_y + height } 
        else { buffer_height }
    }; 

    let buffer = framebuffer.buffer();

    let fill = vec![color; end_x - start_x];
    let mut y = start_y;
    loop {
        if y == end_y {
            return;
        }
        buffer[index(start_x, y)..index(end_x, y)].copy_from_slice(&fill);
        y += 1;
    }
}
