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
