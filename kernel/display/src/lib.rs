#![no_std]

extern crate owning_ref;
extern crate memory;
extern crate alloc;
extern crate frame_buffer;
extern crate font;

use alloc::vec;
use font::{CHARACTER_HEIGHT, CHARACTER_WIDTH, FONT_PIXEL};
use frame_buffer::FrameBuffer;

const COLOR_BITS:u32 = 24;

///draw a pixel
pub fn draw_pixel(mut framebuffer:&mut FrameBuffer, x:usize, y:usize, color:u32){    
    if framebuffer.check_in_range(x, y) {
        write_to(&mut framebuffer, x, y, color);
    }
}

///draw a line from (start_x, start_y) to (end_x, end_y) with color
pub fn draw_line(mut framebuffer:&mut FrameBuffer, start_x:i32, start_y:i32, end_x:i32, end_y:i32, color:u32){
    let width:i32 = end_x - start_x;
    let height:i32 = end_y - start_y;

    //compare the x distance and y distance. Increase/Decrease the longer one at every step.
    if width.abs() > height.abs() {
        let mut y;
        let mut x = start_x;

        //if the end_x is larger than start_x, increase x in the loop. Otherwise decrease it.
        let step = if width > 0 { 1 } else { -1 };
        loop {
            if x == end_x {
                break;
            }          
            y = (x - start_x) * height / width + start_y;
            if framebuffer.check_in_range(x as usize, y as usize) {
                write_to(&mut framebuffer, x as usize, y as usize, color);
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
                write_to(&mut framebuffer, x as usize, y as usize, color);
            }
            y += step;   
        }
    }
}

//draw a rectangle at (start_x, start_y) with color
pub fn draw_rectangle(mut framebuffer:&mut FrameBuffer, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
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
    loop {
        if x == end_x {
            break;
        }
        write_to(&mut framebuffer, x as usize, start_y as usize, color);
        write_to(&mut framebuffer, x as usize, end_y - 1 as usize, color);
        x += 1;
    }

    let mut y = start_y;
    loop {
        if y == end_y {
            break;
        }
        write_to(&mut framebuffer, start_x as usize, y as usize, color);
        write_to(&mut framebuffer, end_x - 1 as usize, y as usize, color);
        y += 1;
    }
}

//fill a rectangle at (start_x, start_y) with color
pub fn fill_rectangle(mut framebuffer:&mut FrameBuffer, start_x:usize, start_y:usize, width:usize, height:usize, color:u32){
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
    let mut y = start_y;
    loop {
        loop {
            write_to(&mut framebuffer, x as usize, y as usize, color);
            x += 1;
            if x == end_x {
                break;
            }
        }
        y += 1;
        if y == end_y {
            break;
        }
        x = start_x;
    }
}

///print a string by bytes at (x, y) within an area of (width, height) of the virtual text frame buffer
pub fn print_by_bytes(mut framebuffer:&mut FrameBuffer, x:usize, y:usize, width:usize, height:usize, 
    slice: &str, font_color:u32, bg_color:u32) -> Result<(), &'static str> {
    let buffer_width = width/CHARACTER_WIDTH;
    let buffer_height = height/CHARACTER_HEIGHT;

    let mut curr_line = 0;
    let mut curr_column = 0;        
    for byte in slice.bytes() {
        if byte == b'\n' {
            //fill the remaining blank of current line and go to the next line
            fill_blank(&mut framebuffer,
                x + curr_column * CHARACTER_WIDTH,
                y + curr_line * CHARACTER_HEIGHT,
                x + width, 
                y + (curr_line + 1 )* CHARACTER_HEIGHT, 
                bg_color)?;
            curr_column = 0;
            curr_line += 1;
            if curr_line == buffer_height {
                break;
            }
        } else {
            if curr_column == buffer_width {
                curr_column = 0;
                curr_line += 1;
                if curr_line == buffer_height {
                    break;
                }
            }
            print_byte(&mut framebuffer, byte, font_color, bg_color, x, y, 
                curr_line, curr_column)?;
            curr_column += 1;
        }
    }
    
    // //Fill the blank of the last line
    // fill_blank(&mut buffer,
    //     x + curr_column * CHARACTER_WIDTH,
    //     y + curr_line * CHARACTER_HEIGHT,
    //     x + width, 
    //     y + (curr_line + 1 )* CHARACTER_HEIGHT, 
    //     bg_color, &index)?;

    // Fill the blank of remaining lines
    // fill_blank(&mut buffer, 
    //     x, y + (curr_line + 1 )* CHARACTER_HEIGHT, x + width, y + height, 
    //     bg_color, &index)?;

    Ok(())
}

//print a byte to the text buffer at (line, column). (left, top) specify the padding of the text area. index is the function to calculate the index of every pixel. The virtual frame buffer calls its get_index() method to get the function.
fn print_byte(framebuffer:&mut FrameBuffer, byte:u8, font_color:u32, bg_color:u32,
        left:usize, top:usize, line:usize, column:usize) 
        -> Result<(),&'static str> {
    let x = left + column * CHARACTER_WIDTH;
    let y = top + line * CHARACTER_HEIGHT;
    let fonts = FONT_PIXEL.lock();

    let mut i = 0;
    let mut j = 0;
    loop {
        let mask:u32 = fonts[byte as usize][i][j];
        let index = framebuffer.index(x + j, y + i);
        framebuffer.buffer()[index] = font_color & mask | bg_color & (!mask);
        j += 1;
        if j == CHARACTER_WIDTH {
            i += 1;
            if i == CHARACTER_HEIGHT {
                return Ok(());
            }
            j = 0;
        }
    }
}

//Fill a blank (left, top, right, bottom) with the color. index is the function to calculate the index of every pixel. The virtual frame buffer calls its get_index() method to get the function.
fn fill_blank(framebuffer:&mut FrameBuffer, left:usize, top:usize, right:usize,
            bottom:usize, color:u32) -> Result<(),&'static str>{
    if left >= right || top >= bottom {
        return Ok(())
    }

    let fill = vec![color; right - left];
    let mut y = top;
    loop {
        if y == bottom {
            return Ok(());
        }
        let start = framebuffer.index(left, y);
        let end = framebuffer.index(right, y);
        framebuffer.buffer()[start..end].copy_from_slice(&fill);
        y += 1;
    }
}

fn write_to(framebuffer:&mut FrameBuffer, x:usize, y:usize, color:u32) {
    let index = framebuffer.index(x, y);
    framebuffer.buffer()[index] = color;
}

fn write_to_3d(framebuffer:&mut FrameBuffer, x:usize, y:usize, z:u8, color:u32) {
    let index = framebuffer.index(x, y);
    let buffer = framebuffer.buffer();
    if (buffer[index] >> COLOR_BITS) <= z as u32 {
        buffer[index] = color;
    }
}