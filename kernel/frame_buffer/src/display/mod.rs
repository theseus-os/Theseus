use super::{BoxRefMut, MappedPages};

pub mod graph_drawer;
pub mod text_printer;

fn write_to(buffer:&mut BoxRefMut<MappedPages, [u32]>, index:usize, color:u32) {
    buffer[index] = color;
}