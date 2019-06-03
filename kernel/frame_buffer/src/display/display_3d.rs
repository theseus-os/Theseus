

fn write_to_3d(buffer:&mut BoxRefMut<MappedPages, [u32]>, index:usize, color:u32) {
    if (buffer[index] >> COLOR_BITS) <= z as u32 {
        buffer[index] = color;
    }
}

fn write_to(buffer:&mut BoxRefMut<MappedPages, [u32]>, index:usize, color:u32) {
    buffer[index] = color;
}