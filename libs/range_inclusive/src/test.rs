extern crate std;
use self::std::vec::Vec;

use crate::*;

#[test]
fn greater_end() {
    let range = RangeInclusive::new(0 , 1);
    assert!(!range.is_empty());
    
    let mut range_elements = Vec::new();
    for r in range.iter() {
        range_elements.push(r);
    }
    assert_eq!(range_elements[0], range.start);
    assert_eq!(range_elements[range_elements.len() - 1], range.end);
    assert_eq!(range_elements.len(), range.end - range.start + 1);


    let range = RangeInclusive::new(10 , 17);
    assert!(!range.is_empty());
    
    let mut range_elements = Vec::new();
    for r in range.iter() {
        range_elements.push(r);
    }
    assert_eq!(range_elements[0], range.start);
    assert_eq!(range_elements[range_elements.len() - 1], range.end);
    assert_eq!(range_elements.len(), range.end - range.start + 1);


    let range = RangeInclusive::new(597 , 982);
    assert!(!range.is_empty());
    
    let mut range_elements = Vec::new();
    for r in range.iter() {
        range_elements.push(r);
    }
    assert_eq!(range_elements[0], range.start);
    assert_eq!(range_elements[range_elements.len() - 1], range.end);
    assert_eq!(range_elements.len(), range.end - range.start + 1);
}

#[test]
fn equal_start_end() {
    let range = RangeInclusive::new(0 , 0);
    assert!(!range.is_empty());
    
    let mut range_elements = Vec::new();
    for r in range.iter() {
        range_elements.push(r);
    }
    assert_eq!(range_elements[0], range.start);
    assert_eq!(range_elements[range_elements.len() - 1], range.end);
    assert_eq!(range_elements.len(), range.end - range.start + 1);
    
    let range = RangeInclusive::new(597 , 597);
    assert!(!range.is_empty());
    
    let mut range_elements = Vec::new();
    for r in range.iter() {
        range_elements.push(r);
    }
    assert_eq!(range_elements[0], range.start);
    assert_eq!(range_elements[range_elements.len() - 1], range.end);
    assert_eq!(range_elements.len(), range.end - range.start + 1);
}

#[test]
fn greater_start() {
    let range = RangeInclusive::new(782 , 597);
    assert!(range.is_empty());
    let mut range_elements = Vec::new();
    for r in range.iter() {
        range_elements.push(r);
    }
    assert!(range_elements.is_empty());
    
    let range = RangeInclusive::new(1 , 0);
    assert!(range.is_empty());
    let mut range_elements = Vec::new();
    for r in range.iter() {
        range_elements.push(r);
    }
    assert!(range_elements.is_empty());
}

#[test]
fn other_iterators() {
    let range = RangeInclusive::new(1 , 6);
    let mut iter = range.iter();
    assert_eq!(iter.len(), 6);
    assert_eq!(Some(1), iter.next());
    
    assert_eq!(iter.len(), 5);
    assert_eq!(Some(6), iter.next_back());
    
    assert_eq!(iter.len(), 4);
    assert_eq!(Some(5), iter.next_back());
    
    assert_eq!(iter.len(), 3);
    assert_eq!(Some(2), iter.next());
    
    assert_eq!(iter.len(), 2);
    assert_eq!(Some(3), iter.next());
    
    assert_eq!(iter.len(), 1);
    assert_eq!(Some(4), iter.next());
    
    assert_eq!(iter.len(), 0);
    assert_eq!(None, iter.next());
    
    assert_eq!(iter.len(), 0);
    assert_eq!(None, iter.next_back());
    
    assert_eq!(iter.len(), 0);
}
