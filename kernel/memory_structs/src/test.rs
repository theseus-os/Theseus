//! Tests sized variations of core paging types

extern crate std;

use super::*;

#[test]
fn huge_2mb_range_size() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x400000 - 1).unwrap()));

    assert_eq!(r.end().number(), 512);
    assert_eq!(r.start().number(), 512);
    assert_eq!(r.size_in_pages(), 1);
}

#[test]
fn huge_2mb_range_size2() {
    let r: PageRange<Page2MiB> = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x800000 - 1).unwrap()));
    
    assert_eq!(r.end().number(), 1536);
    assert_eq!(r.start().number(), 512);
    assert_eq!(r.size_in_pages(), 3);
}

#[test]
fn huge_2mb_range_iteration1() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x400000 - 1).unwrap()));
    let mut num_iters = 0;
    for _ in r {
        num_iters += 1;
    }
    assert_eq!(num_iters, 1);
}

#[test]
fn huge_2mb_range_iteration2() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x800000 - 1).unwrap()));
    let mut num_iters = 0;
    // assert_eq!(r.start().number, 0x200000);
    for _ in r {
        num_iters += 1;
    }
    assert_eq!(num_iters, 3);
}

#[test]
fn huge_2mb_range_iteration3() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x1000000 - 1).unwrap()));
    let mut num_iters = 0;
    for _ in r {
        num_iters += 1;
    }
    assert_eq!(num_iters, 7);
}

#[test]
fn huge_1gb_range_iteration() {
    let r = PageRange::<Page1GiB>::new(
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x40000000).unwrap()),
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x80000000 - 1).unwrap()));
    let mut num_iters = 0;
    for _ in r {
        num_iters += 1;
    }
    assert_eq!(num_iters, 1);
}

#[test]
fn huge_1gb_range_iteration2() {
    let r = PageRange::<Page1GiB>::new(
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x40000000).unwrap()),
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x100000000 - 1).unwrap()));
    assert_eq!(r.size_in_pages(), 3);
    
    let mut num_iters = 0;
    for _ in r {
        num_iters += 1;
    }
    assert_eq!(num_iters, 3);
}

#[test]
fn huge_1gb_from_4kb() {
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x40000000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x80000000 - 1).unwrap()));


    let new1gb = PageRange::<Page1GiB>::try_from(r).unwrap();

    assert!(matches!(new1gb.start().page_size(), MemChunkSize::Huge1G));
    
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x40000000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x42000000).unwrap()));

    let new1gb = PageRange::<Page1GiB>::try_from(r);
    assert!(new1gb.is_err());
}

#[test]
fn huge_2gb_from_4kb() {
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x200000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x800000 - 1).unwrap()));

    let new2mb = PageRange::<Page2MiB>::try_from(r).unwrap(); // r.into_2mb_range().unwrap();
    assert!(matches!(new2mb.start().page_size(), MemChunkSize::Huge2M));

    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x30000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x40000).unwrap()));

    let new2mb = PageRange::<Page2MiB>::try_from(r);
    assert!(new2mb.is_err());
}

#[test]
fn standard_sized_from_1gb() {
    let r = PageRange::<Page1GiB>::new(
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x40000000).unwrap()),
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x48000000 - 1).unwrap()));

    // Compiler needs to the size to be explicitly provided
    let converted = PageRange::<Page4KiB>::from(r);

    assert!(matches!(converted.start().page_size(), MemChunkSize::Normal4K));
}

#[test]
fn standard_sized_from_2mb() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x40000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x80000 - 1).unwrap()));

    let converted = PageRange::<Page4KiB>::from(r);

    assert!(matches!(converted.start().page_size(), MemChunkSize::Normal4K));
}

#[test]
fn try_from_conversions() {
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x200000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x800000 - 1).unwrap()));
    assert_eq!(r.size_in_pages(), 1536);
    
    let new2mb = PageRange::<Page2MiB>::try_from(r).unwrap();
    assert!(matches!(new2mb.start().page_size(), MemChunkSize::Huge2M));
    assert_eq!(new2mb.size_in_pages(), 3);
    
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x40000000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x80000000 - 1).unwrap()));
    assert_eq!(r.size_in_pages(), 262144);

    let new1gb = PageRange::<Page1GiB>::try_from(r).unwrap();
    assert!(matches!(new1gb.start().page_size(), MemChunkSize::Huge1G));
    assert_eq!(new1gb.size_in_pages(), 1);
}
