//! Tests for the AllocatedFrames type, mainly the `split` method.
//! These tests have to be run individually because running them all at once leads to overlaps between `TrustedChunk`s
//! which will return an error.

extern crate std;

use self::std::dbg;

use super::*;

fn from_addr(start_addr: usize, end_addr: usize) -> AllocatedFrames {
    AllocatedFrames::new(MemoryRegionType::Free, FrameRange::new(
            Frame::containing_address(PhysicalAddress::new_canonical(start_addr)),
            Frame::containing_address(PhysicalAddress::new_canonical(end_addr)),
        )).unwrap()
}

fn frame_addr(addr: usize) -> Frame {
    Frame::containing_address(PhysicalAddress::new_canonical(addr))
}

#[test]
fn split_before_beginning() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(0x4274000);

    let result = original.split_at(split_at);
    dbg!(&result);
    assert!(result.is_err());
}

#[test]
fn split_at_beginning() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(0x4275000);
    let first    = FrameRange::empty();
    let second   = FrameRange::new(frame_addr(0x4275000), frame_addr(0x4285000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}


#[test]
fn split_at_middle() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(     0x427D000);
    let first    = FrameRange::new(frame_addr(0x4275000), frame_addr(0x427C000));
    let second   = FrameRange::new( frame_addr(0x427D000), frame_addr(0x4285000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}

#[test]
fn split_at_end() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(           0x4285000);
    let first    = FrameRange::new( frame_addr(0x4275000), frame_addr(0x4284000));
    let second   = FrameRange::new( frame_addr(0x4285000), frame_addr(0x4285000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}


#[test]
fn split_after_end() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(           0x4286000);
    let first    = FrameRange::new( frame_addr(0x4275000), frame_addr(0x4285000));
    let second   = FrameRange::empty();

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}


#[test]
fn split_empty_at_zero() {
    let original = AllocatedFrames::empty();
    let split_at = frame_addr(0x0000);

    let result = original.split_at(split_at);
    dbg!(&result);
    assert!(result.is_err());
}

#[test]
fn split_empty_at_one() {
    let original = AllocatedFrames::empty();
    let split_at = frame_addr(0x1000);

    let result = original.split_at(split_at);
    dbg!(&result);
    assert!(result.is_err());
}

#[test]
fn split_empty_at_two() {
    let original = AllocatedFrames::empty();
    let split_at = frame_addr(0x2000);

    let result = original.split_at(split_at);
    dbg!(&result);
    assert!(result.is_err());
}



#[test]
fn split_at_beginning_zero() {
    let original = from_addr( 0x0, 0x5000);
    let split_at = frame_addr(0x0); // leads to attempt to subtract with overflow
    let first  = FrameRange::empty();
    let second = FrameRange::new(frame_addr(0x0), frame_addr(0x5000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}

#[test]
fn split_at_beginning_one() {
    let original = from_addr( 0x0000, 0x5000);
    let split_at = frame_addr(0x1000);
    let first    = FrameRange::new( frame_addr(0x0000), frame_addr(0x0000));
    let second   = FrameRange::new( frame_addr(0x1000), frame_addr(0x5000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}

#[test]
fn split_at_beginning_max_length_one() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_F000, 0xFFFF_FFFF_FFFF_F000);
    let split_at = frame_addr(0xFFFF_FFFF_FFFF_F000);
    let first    = FrameRange::empty();
    let second   = FrameRange::new(frame_addr(0xFFFF_FFFF_FFFF_F000), frame_addr(0xFFFF_FFFF_FFFF_F000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}

#[test]
fn split_at_end_max_length_two() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_F000);
    let split_at = frame_addr(                       0xFFFF_FFFF_FFFF_F000);
    let first    = FrameRange::new( frame_addr(0xFFFF_FFFF_FFFF_E000), frame_addr(0xFFFF_FFFF_FFFF_E000));
    let second   = FrameRange::new( frame_addr(0xFFFF_FFFF_FFFF_F000), frame_addr(0xFFFF_FFFF_FFFF_F000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}


#[test]
fn split_after_end_max() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_E000);
    let split_at = frame_addr(0xFFFF_FFFF_FFFF_F000);
    let first  =   FrameRange::new( frame_addr(0xFFFF_FFFF_FFFF_E000), frame_addr(0xFFFF_FFFF_FFFF_E000));
    let second =   FrameRange::empty();

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}

#[test]
fn split_at_beginning_max() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_E000);
    let split_at = frame_addr(0xFFFF_FFFF_FFFF_E000);
    let first    = FrameRange::empty();
    let second   = FrameRange::new(frame_addr(0xFFFF_FFFF_FFFF_E000), frame_addr(0xFFFF_FFFF_FFFF_E000));

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.deref().clone(), first);
    assert_eq!(result2.deref().clone(), second);
}
