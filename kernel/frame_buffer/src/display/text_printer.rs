extern crate font;

use super::write_to;
use super::super::{FrameBuffer, Box, MappedPages, BoxRefMut, hpet};
use self::font::*;

///print a string by bytes at (x, y) within an area of (width, height) of the virtual text frame buffer
pub fn print_by_bytes(mut framebuffer:&mut FrameBuffer, x:usize, y:usize, width:usize, height:usize, 
    slice: &str, font_color:u32, bg_color:u32) -> Result<(), &'static str> {

    let mut curr_line = 0;
    let mut curr_column = 0;

    let buffer_width = width/CHARACTER_WIDTH;
    let buffer_height = height/CHARACTER_HEIGHT;
    
    let index = framebuffer.get_index_fn();

    let mut buffer = framebuffer.buffer();
        
    for byte in slice.bytes() {
        if byte == b'\n' {//fill the remaining blank of current line and go to the next line
            fill_blank(&mut buffer,
                x + curr_column * CHARACTER_WIDTH,
                y + curr_line * CHARACTER_HEIGHT,
                x + width, 
                y + (curr_line + 1 )* CHARACTER_HEIGHT, 
                bg_color, &index)?;
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
            print_byte(&mut buffer, byte, font_color, bg_color, x, y, 
                curr_line, curr_column, &index)?;
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
fn print_byte(buffer:&mut BoxRefMut<MappedPages, [u32]>, byte:u8, font_color:u32, bg_color:u32,
        left:usize, top:usize, line:usize, column:usize, index:&Box<Fn(usize, usize)->usize>) 
        -> Result<(),&'static str> {
    let x = left + column * CHARACTER_WIDTH;
    let y = top + line * CHARACTER_HEIGHT;
    let mut i = 0;
    let mut j = 0;

    let fonts = FONT_PIXEL.lock();

    loop {
        let mask:u32 = fonts[byte as usize][i][j];
        buffer[index(x + j, y + i)] = font_color & mask | bg_color & (!mask);
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
fn fill_blank(buffer:&mut BoxRefMut<MappedPages, [u32]>, left:usize, top:usize, right:usize,
            bottom:usize, color:u32, index:&Box<Fn(usize, usize)->usize>) -> Result<(),&'static str>{
    if left >= right || top >= bottom {
        return Ok(())
    }
    let fill = vec![color; right - left];
    let mut y = top;
    loop {
        if y == bottom {
            return Ok(());
        }
        buffer[index(left, y)..index(right, y)].copy_from_slice(&fill);
        y += 1;
    }
}