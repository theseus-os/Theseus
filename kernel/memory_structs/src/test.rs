//! Tests sized variations of core paging types

extern crate std;

use self::std::dbg;

use super::*;

#[test]
fn huge_2mb_range_size() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x400000).unwrap()));

    assert_eq!(r.size_in_pages(), 1);
}

#[test]
fn huge_2mb_range_size2() {
    let r: PageRange<Page2MiB> = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x800000).unwrap()));
    assert_eq!(r.size_in_pages(), 3);
}

#[test]
fn huge_2mb_range_iteration1() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x400000).unwrap()));
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
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x800000).unwrap()));
    let mut num_iters = 0;
    for _ in r {
        num_iters += 1;
    }
    assert_eq!(num_iters, 3);
}

#[test]
fn huge_2mb_range_iteration3() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x200000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x1000000).unwrap()));
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
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x80000000).unwrap()));
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
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x100000000).unwrap()));
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
        Page::containing_address(VirtualAddress::new(0x80000000).unwrap()));


    let new1gb = r.into_1gb_range().unwrap();

    assert!(matches!(new1gb.start().page_size(), MemChunkSize::Huge1G));
    
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x40000000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x42000000).unwrap()));

    let new1gb = r.into_1gb_range();
    assert_eq!(new1gb, None);
}

#[test]
fn huge_2gb_from_4kb() {
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x200000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x800000).unwrap()));

    let new2mb = r.into_2mb_range().unwrap();
    assert!(matches!(new2mb.start().page_size(), MemChunkSize::Huge2M));

    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x30000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x40000).unwrap()));

    let new2mb = r.into_2mb_range();
    assert_eq!(new2mb, None);
}

#[test]
fn standard_sized_from_1gb() {
    let r = PageRange::<Page1GiB>::new(
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x40000000).unwrap()),
        Page::<Page1GiB>::containing_address_1gb(VirtualAddress::new(0x48000000).unwrap()));

    let converted = r.as_4kb_range();

    assert!(matches!(converted.start().page_size(), MemChunkSize::Normal4K));
}

#[test]
fn standard_sized_from_2mb() {
    let r = PageRange::<Page2MiB>::new(
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x40000).unwrap()),
        Page::<Page2MiB>::containing_address_2mb(VirtualAddress::new(0x80000).unwrap()));

    let converted = r.as_4kb_range();

    assert!(matches!(converted.start().page_size(), MemChunkSize::Normal4K));
}

#[test]
fn try_from_conversions() {
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x200000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x800000).unwrap()));

    let new2mb = PageRange::<Page2MiB>::try_from(r).unwrap();
    assert!(matches!(new2mb.start().page_size(), MemChunkSize::Huge2M));
    
    let r = PageRange::new(
        Page::containing_address(VirtualAddress::new(0x40000000).unwrap()),
        Page::containing_address(VirtualAddress::new(0x80000000).unwrap()));

    let new1gb = PageRange::<Page1GiB>::try_from(r).unwrap();
    assert!(matches!(new1gb.start().page_size(), MemChunkSize::Huge1G));
}
