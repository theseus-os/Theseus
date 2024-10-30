use crate::*;
use crate::trusted_chunk::*;

#[test]
fn chunk_allocator_test() {
    let mut allocator = TrustedChunkAllocator::new();
    let mut chunk;
    chunk = allocator.create_chunk(RangeInclusive::new(0,1));
    assert!(chunk.is_ok());
    chunk = allocator.create_chunk(RangeInclusive::new(0,1)); // equivalent
    assert!(chunk.is_err());
    chunk = allocator.create_chunk(RangeInclusive::new(5,1)); // empty range
    assert!(chunk.is_err());
    chunk = allocator.create_chunk(RangeInclusive::new(4,10)); // disjoint
    assert!(chunk.is_ok());
    chunk = allocator.create_chunk(RangeInclusive::new(10,12)); // upper range bound overlap
    assert!(chunk.is_err());
    chunk = allocator.create_chunk(RangeInclusive::new(3,4)); // lower range bound overlap
    assert!(chunk.is_err());

    assert_eq!(allocator.array.lookup(0), Some(RangeInclusive::new(0,1)));
    assert_eq!(allocator.array.lookup(1), Some(RangeInclusive::new(4,10)));

    for i in 2..allocator.array.len() {
        assert_eq!(allocator.array.lookup(i), None);
    }

    let res = allocator.switch_to_heap_allocated();
    assert!(res.is_ok());

    let res = allocator.switch_to_heap_allocated();
    assert!(res.is_err());

    // for i in 0..allocator.array.len() {
    //     assert_eq!(allocator.array.lookup(i), None);
    // }

    assert_eq!(allocator.list.len(), 2);
    assert_eq!(allocator.list.lookup(1), RangeInclusive::new(0,1));
    assert_eq!(allocator.list.lookup(0), RangeInclusive::new(4,10));

    chunk = allocator.create_chunk(RangeInclusive::new(7,12)); // multiple overlap
    assert!(chunk.is_err());
    chunk = allocator.create_chunk(RangeInclusive::new(10, 11)); // start range bound overlap
    assert!(chunk.is_err());
    chunk = allocator.create_chunk(RangeInclusive::new(11, 11));
    assert!(chunk.is_ok());
    chunk = allocator.create_chunk(RangeInclusive::new(11, 15)); // end range bound overlap
    assert!(chunk.is_err());
    chunk = allocator.create_chunk(RangeInclusive::new(1, 0)); // empty
    assert!(chunk.is_err());
    chunk = allocator.create_chunk(RangeInclusive::new(12, 15));
    assert!(chunk.is_ok());

    assert_eq!(allocator.list.len(), 4);
    assert_eq!(allocator.list.lookup(3), RangeInclusive::new(0,1));
    assert_eq!(allocator.list.lookup(2), RangeInclusive::new(4,10));
    assert_eq!(allocator.list.lookup(1), RangeInclusive::new(11,11));
    assert_eq!(allocator.list.lookup(0), RangeInclusive::new(12,15));
}
