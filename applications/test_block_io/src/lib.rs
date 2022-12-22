//! Simple tests for block-wise and byte-wise I/O traits and wrappers.
//!
//! Currently only the [`ata::AtaDrive`] implementation is available, so that's what is tested.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
// #[macro_use] extern crate app_io;
extern crate task;
extern crate io;
extern crate core2;
extern crate storage_manager;
extern crate ata;


use core::ops::{DerefMut};

use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::string::String;
use ata::AtaDrive;
use io::{ByteReader, ByteReaderWrapper, ByteReaderWriterWrapper, ByteWriter, ByteWriterWrapper, Reader, ReaderWriter};


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
    // let bytes_read = ByteReaderWrapper(&mut *dev.lock()).read_at(&mut buf[..], 345).unwrap();
    let bytes_read = ByteReaderWrapper::from(dev.lock().deref_mut())
        .read_at(&mut buf[..], 345).unwrap();
    trace!("After ByteReader test: read {} bytes starting at {}:\n{:X?}", bytes_read, 345, &buf[..]);

    if &after_buf[345..345+1699] == buf {
        info!("ByteReader example worked");
    } else {
        error!("ByteReader example failed");
    }

    // Test the ByteWriter trait, then read it back to confirm
    let buf_to_write = b"HELLO WORLD!";
    let bytes_written = ByteWriterWrapper::from(&mut *dev.lock()).write_at(buf_to_write, 720).unwrap();
    let mut new_buf: [u8; 1000] = [0; 1000];
    let bytes_read = ByteReaderWrapper::from(dev.lock().deref_mut()).read_at(&mut new_buf, 600).unwrap();
    trace!("After ByteWriter test: read {} bytes at {} after writing {} bytes:\n{:X?}", bytes_read, 600, bytes_written, &new_buf[..]);
    if &new_buf[120..132] == buf_to_write {
        info!("ByteWriter example worked");
    } else {
        error!("ByteWriter example failed");
    }


    // Test the Reader, Writer, ReaderWriter stuff (with offsets)
    use core2::io::{Read, Write, Seek, SeekFrom};
    let mut locked_sd = dev.lock();
    // let dev_mut = locked_sd.deref_mut();
    {
        let downcasted: Option<&mut ata::AtaDrive> = locked_sd.as_any_mut().downcast_mut();
        if let Some(ata_drive) = downcasted {
            // a quick unsafe cheater method to obtain an owned storage drive instance.
            let owned_drive:  AtaDrive = unsafe { core::ptr::read(ata_drive as *mut AtaDrive as *const AtaDrive) };
            let owned_drive2: AtaDrive = unsafe { core::ptr::read(ata_drive as *mut AtaDrive as *const AtaDrive) };

            let mut my_buf: [u8; 10] = [0; 10];
            let mut ata_drive = ByteReaderWrapper::from(ata_drive);
            ByteReader::read_at(&mut ata_drive, &mut my_buf, 0x20).unwrap();
            trace!("Here1: my_buf: {:X?}", my_buf);
            ata_drive.read_at(&mut my_buf[5..], 0x30).unwrap();
            trace!("Here2: my_buf: {:X?}", my_buf);
            
            let mut owned_drive = ByteReaderWrapper::from(owned_drive);
            owned_drive.read_at(&mut my_buf, 0x50).unwrap();
            trace!("Here3: my_buf: {:X?}", my_buf);
            let mut reader = Reader::new(owned_drive);
            let bytes_read  = reader.read(&mut my_buf[1..5]).unwrap();
            assert_eq!(bytes_read, 4);
            trace!("Here4: my_buf: {:X?}", my_buf);
            
            let bytes_read2 = reader.read(&mut my_buf[5..9]).unwrap();
            assert_eq!(bytes_read2, 4);
            trace!("Here5: my_buf: {:X?}", my_buf);
            

            // test accepting a boxed reader
            let mut reader = Reader::new(Box::new(ByteReaderWrapper::from(owned_drive2)) as Box<dyn ByteReader>);
            let bytes_read3 = reader.read(&mut my_buf[5..]).unwrap();
            assert_eq!(bytes_read3, 5);
            trace!("Here6: my_buf: {:X?}", my_buf);
            
        }
    }

    let mut io = ReaderWriter::new(ByteReaderWriterWrapper::from(&mut *locked_sd));
    let mut bb = [0u8; 100];
    let bread = io.read(&mut bb).unwrap();
    info!("Final ReaderWriter test: read {} bytes into bb: {:X?}", bread, bb);
    let start_offset = io.seek(SeekFrom::Current(0)).unwrap();
    let bwritten = io.write(b"YO WHAT IS UP").unwrap();
    let end_offset = io.seek(SeekFrom::Current(0)).unwrap();
    info!("Final ReaderWriter test: wrote {} bytes from offset {}..{}", bwritten, start_offset, end_offset);
    
    let mut read_buf = [0u8; 50];
    let pos = io.seek(SeekFrom::Current(-50)).unwrap();
    let bread = io.read(&mut read_buf).unwrap();
    info!("Final ReaderWriter test: read back {} bytes from offset {} into read_buf: {:X?}", bread, pos, read_buf);


    0
}
