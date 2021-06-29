#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
// #[macro_use] extern crate terminal_print;
extern crate task;
extern crate block_io;
extern crate bare_io;
extern crate storage_manager;
extern crate ata;


use core::ops::DerefMut;

use alloc::vec::Vec;
use alloc::string::String;
use ata::AtaDrive;
use block_io::{ByteReader, ByteWriter, Reader, ReaderWriter};


pub fn main(_args: Vec<String>) -> isize {

    let dev = storage_manager::storage_devices().next()
        .expect("no storage devices exist");

    {
        // Call `StorageDevice` trait methods directly
        let mut locked_sd = dev.lock();
        debug!("Found drive with size {}, {} sectors", locked_sd.len(), locked_sd.size_in_blocks());
        // Here we downcast the `StorageDevice` into an `AtaDrive` so we can call `AtaDrive` methods.
        let downcasted: Option<&mut ata::AtaDrive> = locked_sd.as_any_mut().downcast_mut();
        if let Some(ata_drive) = downcasted {
            debug!("      drive was master? {}", ata_drive.is_master());
            // Read 10 sectors from the beginning of the drive (at offset 0)
            let mut initial_buf: [u8; 5120] = [0; 5120]; // 10 sectors of bytes
            let sectors_read = ata_drive.read_pio(&mut initial_buf[..], 0).unwrap();
            debug!("[SUCCESS] sectors_read: {:?}", sectors_read);
            debug!("{:?}", core::str::from_utf8(&initial_buf));
        }
    }
    // Read 10 sectors from the drive using the `StorageDevice` trait methods.
    let mut after_buf: [u8; 5120] = [0; 5120];
    let sectors_read = dev.lock().read_blocks(&mut after_buf[..], 0).unwrap();
    debug!("{:X?}", &after_buf[..]);
    debug!("{:?}", core::str::from_utf8(&after_buf));
    trace!("POST-WRITE READ_BLOCKS {} sectors", sectors_read);


    // Test the ByteReader traits
    let mut buf: [u8; 1699] = [0; 1699];
    let bytes_read = dev.lock()
        .read_at(&mut buf[..], 345).unwrap();
    trace!("After ByteReader test: read {} bytes starting at {}:\n{:X?}", bytes_read, 345, &buf[..]);

    if &after_buf[345..345+1699] == buf {
        info!("ByteReader example worked");
    } else {
        error!("ByteReader example failed");
    }

    // Test the ByteWriter trait, then read it back to confirm
    let buf_to_write = b"HELLO WORLD!";
    let bytes_written = dev.lock().write_at(buf_to_write, 720).unwrap();
    let mut new_buf: [u8; 1000] = [0; 1000];
    let bytes_read = dev.lock().read_at(&mut new_buf, 600).unwrap();
    trace!("After ByteWriter test: read {} bytes at {} after writing {} bytes:\n{:X?}", bytes_read, 600, bytes_written, &new_buf[..]);
    if &new_buf[120..132] == buf_to_write {
        info!("ByteWriter example worked");
    } else {
        error!("ByteWriter example failed");
    }


    // TODO: here test the Reader, Writer, ReaderWriter stuff (with offsets)
    let mut locked_sd = dev.lock();
    // let dev_mut = locked_sd.deref_mut();
    {
        let downcasted: Option<&mut ata::AtaDrive> = locked_sd.as_any_mut().downcast_mut();
        if let Some(ata_drive) = downcasted {
            let mut my_buf: [u8; 10] = [0; 10];
            ByteReader::read_at(ata_drive, &mut my_buf, 0x20).unwrap();
            trace!("Here1: my_buf: {:X?}", my_buf);
            ata_drive.read_at(&mut my_buf[5..], 0x30).unwrap();
            trace!("Here2: my_buf: {:X?}", my_buf);
            
            let mut owned_drive: AtaDrive = unsafe { core::ptr::read(ata_drive as *mut _ as *const _) };
            owned_drive.read_at(&mut my_buf, 0x50).unwrap();
            trace!("Here3: my_buf: {:X?}", my_buf);
            let mut reader = Reader::new(owned_drive);
            use bare_io::Read;
            let bytes_read  = reader.read(&mut my_buf[1..5]).unwrap();
            assert_eq!(bytes_read, 4);
            trace!("Here4: my_buf: {:X?}", my_buf);

            let bytes_read2 = reader.read(&mut my_buf[5..9]).unwrap();
            assert_eq!(bytes_read2, 4);
            trace!("Here5: my_buf: {:X?}", my_buf);


            // let reader = Reader::new(ata_drive);

        }
    }

    // TODO: fix the `Reader` struct's trait bounds to accept a `ByteReader` or a `&mut ByteReader`
    // let rw = Reader::new(&mut locked_sd); // THIS DOESN'T COMPILE

    0
}
