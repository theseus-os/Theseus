//! Tests for the `Frames` type, mainly the `split` method.

extern crate std;

use self::std::dbg;

use super::*;

fn from_addr(start_addr: usize, end_addr: usize) -> FreeFrames {
    FreeFrames::new(
        MemoryRegionType::Free,
        FrameRange::new(
            Frame::containing_address(PhysicalAddress::new_canonical(start_addr)),
            Frame::containing_address(PhysicalAddress::new_canonical(end_addr)),
        )
    )
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
    let first    = AllocatedFrames::empty();
    let second   = from_addr( 0x4275000, 0x4285000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}


#[test]
fn split_at_middle() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(     0x427D000);
    let first    = from_addr( 0x4275000, 0x427C000);
    let second   = from_addr( 0x427D000, 0x4285000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}

#[test]
fn split_at_end() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(           0x4285000);
    let first    = from_addr( 0x4275000, 0x4284000);
    let second   = from_addr( 0x4285000, 0x4285000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}


#[test]
fn split_after_end() {
    let original = from_addr( 0x4275000, 0x4285000);
    let split_at = frame_addr(           0x4286000);
    let first    = from_addr( 0x4275000, 0x4285000);
    let second   = AllocatedFrames::empty();

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
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
    let split_at = frame_addr(0x0);
    let first  = AllocatedFrames::empty();
    let second = from_addr(0x0, 0x5000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}

#[test]
fn split_at_beginning_one() {
    let original = from_addr( 0x0000, 0x5000);
    let split_at = frame_addr(0x1000);
    let first    = from_addr( 0x0000, 0x0000);
    let second   = from_addr( 0x1000, 0x5000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}

#[test]
fn split_at_beginning_max_length_one() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_F000, 0xFFFF_FFFF_FFFF_F000);
    let split_at = frame_addr(0xFFFF_FFFF_FFFF_F000);
    let first    = AllocatedFrames::empty();
    let second   = from_addr(0xFFFF_FFFF_FFFF_F000, 0xFFFF_FFFF_FFFF_F000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}

#[test]
fn split_at_end_max_length_two() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_F000);
    let split_at = frame_addr(                       0xFFFF_FFFF_FFFF_F000);
    let first    = from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_E000);
    let second   = from_addr( 0xFFFF_FFFF_FFFF_F000, 0xFFFF_FFFF_FFFF_F000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}


#[test]
fn split_after_end_max() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_E000);
    let split_at = frame_addr(0xFFFF_FFFF_FFFF_F000);
    let first  =   from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_E000);
    let second =   AllocatedFrames::empty();

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}

#[test]
fn split_at_beginning_max() {
    let original = from_addr( 0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_E000);
    let split_at = frame_addr(0xFFFF_FFFF_FFFF_E000);
    let first    = AllocatedFrames::empty();
    let second   = from_addr(0xFFFF_FFFF_FFFF_E000, 0xFFFF_FFFF_FFFF_E000);

    let result = original.split_at(split_at);
    dbg!(&result);
    let (result1, result2) = result.unwrap();
    assert_eq!(result1.start(), first.start());
    assert_eq!(result1.end(), first.end());
    assert_eq!(result2.start(), second.start());
    assert_eq!(result2.end(), second.end());
}
