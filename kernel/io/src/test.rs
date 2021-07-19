//! Unit tests for the [`super::blocks_from_bytes()`] function.

extern crate std;
use super::*;

/// A test vector for `blocks_from_bytes()` where both the starting byte and ending byte
/// are not block-aligned.
#[test]
fn test_blockwise_bytewise_multiple_both_unaligned() {
    let transfers = blocks_from_bytes(1500..3950, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 1500..1536,
            block_range: 2..3,
            bytes_in_block_range: 476..512,
        }),
        Some(BlockByteTransfer {
            byte_range_absolute: 1536..3584,
            block_range: 3..7,
            bytes_in_block_range: 0..2048,
        }),
        Some(BlockByteTransfer {
            byte_range_absolute: 3584..3950,
            block_range: 7..8,
            bytes_in_block_range: 0..366,
        }),
    ]);
}

/// A test vector for `blocks_from_bytes()` where 
/// multiple blocks are transferred, with an unaligned start and an aligned end. 
#[test]
fn test_blockwise_bytewise_multiple_unaligned_to_aligned() {
    let transfers = blocks_from_bytes(1693..6144, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 1693..2048,
            block_range: 3..4,
            bytes_in_block_range: 157..512,
        }),
        Some(BlockByteTransfer {
            byte_range_absolute: 2048..6144,
            block_range: 4..12,
            bytes_in_block_range: 0..4096,
        }),
        None,
    ]);
}

/// A test vector for `blocks_from_bytes()` where 
/// multiple blocks are transferred, with an aligned start and an unaligned end. 
#[test]
fn test_blockwise_bytewise_multiple_aligned_to_unaligned() {
    let transfers = blocks_from_bytes(1536..6100, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 1536..5632,
            block_range: 3..11,
            bytes_in_block_range: 0..4096,
        }),
        Some(BlockByteTransfer {
            byte_range_absolute: 5632..6100,
            block_range: 11..12,
            bytes_in_block_range: 0..468,
        }),
        None,
    ]);
}

/// A test vector for `blocks_from_bytes()` where the byte range is within one block.
/// This tests all four combinations of byte alignment within one block:
/// 1. unalighed start, unaligned end
/// 2. aligned start, unaligned end
/// 3. unaligned start, aligned end
/// 4. aligned start, aligned end
#[test]
fn test_blockwise_bytewise_one_block() {
    // 1. unalighed start, unaligned end
    let transfers = blocks_from_bytes(555..900, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 555..900,
            block_range: 1..2,
            bytes_in_block_range: 43..388,
        }),
        None,
        None,
    ]);

    // 2. aligned start, unaligned end
    let transfers = blocks_from_bytes(512..890, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 512..890,
            block_range: 1..2,
            bytes_in_block_range: 0..378,
        }),
        None,
        None,
    ]);

    // 3. unaligned start, aligned end
    let transfers = blocks_from_bytes(671..1024, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 671..1024,
            block_range: 1..2,
            bytes_in_block_range: 159..512,
        }),
        None,
        None,
    ]);

    // 4. aligned start, aligned end
    let transfers = blocks_from_bytes(1024..1536, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 1024..1536,
            block_range: 2..3,
            bytes_in_block_range: 0..512,
        }),
        None,
        None,
    ]);
}

/// A test vector for `blocks_from_bytes()` where
/// the byte range is several blocks, perfectly aligned on both sides. 
#[test]
fn test_blockwise_bytewise_multiple_both_aligned() {
    let transfers = blocks_from_bytes(1024..3072, 512);
    assert_eq!(transfers, [
        Some(BlockByteTransfer {
            byte_range_absolute: 1024..3072,
            block_range: 2..6,
            bytes_in_block_range: 0..2048,
        }),
        None,
        None,
    ]);
}
