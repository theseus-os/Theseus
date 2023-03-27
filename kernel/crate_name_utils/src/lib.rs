//! Utility functions for parsing and processing crate names in symbol strings. 

#![no_std]

#[macro_use] extern crate alloc;
extern crate itertools;
extern crate path;
extern crate crate_metadata;

use core::ops::Range;
use alloc::{
    string::String, 
    vec::Vec,
};
use itertools::Itertools;
use path::Path;
use crate_metadata::CrateType;



/// Returns the crate name that is derived from a crate object file path.
/// 
/// # Examples of acceptable paths
/// Legal paths can:
/// * be absolute or relative,
/// * optionally end with an extension, e.g., `".o"`,   optionally start 
/// * optionally start with a module file prefix, e.g., `"k#my_crate-<hash>.o"`.
pub fn crate_name_from_path(object_file_path: &Path) -> &str {
    let stem = object_file_path.file_stem();
    if let Ok((_crate_type, _prefix, name)) = CrateType::from_module_name(stem) {
        name
    } else {
        stem
    }
}

/// Crate names must be only alphanumeric characters, an underscore, or a dash.
///  
/// See: <https://www.reddit.com/r/rust/comments/4rlom7/what_characters_are_allowed_in_a_crate_name/>
pub fn is_valid_crate_name_char(c: char) -> bool {
    char::is_alphanumeric(c) || 
    c == '_' || 
    c == '-'
}

/// Parses the given symbol string to try to find the name of the parent crate
/// that contains the symbol. 
/// Depending on the symbol, there may be multiple potential parent crates;
/// if so, they are returned in order of likeliness: 
/// the first crate name in the symbol is most likely to contain it.
/// If the parent crate cannot be determined (e.g., a `no_mangle` symbol),
/// then an empty `Vec` is returned.
/// 
/// # Examples
/// * `<*const T as core::fmt::Debug>::fmt` -> `["core"]`
/// * `<alloc::boxed::Box<T>>::into_unique` -> `["alloc"]`
/// * `<framebuffer::VirtualFramebuffer as display::Display>::fill_rectangle` -> `["framebuffer", "display"]`
/// * `keyboard::init` -> `["keyboard"]`
pub fn get_containing_crate_name(demangled_full_symbol: &str) -> Vec<&str> {
    get_containing_crate_name_ranges(demangled_full_symbol)
        .into_iter()
        .filter_map(|range| demangled_full_symbol.get(range))
        .dedup()
        .collect()
}


/// Same as [`get_containing_crate_name()`](#get_containing_crate_name),
/// but returns the substring `Range`s of where the parent crate names 
/// are located in the given `demangled_full_symbol` string.
/// 
/// # Examples
/// * `<*const T as core::fmt::Debug>::fmt` -> `[12..16]`
/// * `<alloc::boxed::Box<T>>::into_unique` -> `[1..6]`
/// * `<framebuffer::VirtualFramebuffer as display::Display>::fill_rectangle` -> `[1..13, 37..44]`
/// * `keyboard::init` -> `[0..8]`
pub fn get_containing_crate_name_ranges(demangled_full_symbol: &str) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    // the separator between independent parts of the symbol string
    const AS: &str = " as ";
    // the index at which we are starting our search for a crate name
    let mut beginning_bound = Some(0);

    while let Some(beg) = beginning_bound {
        // Find the first occurrence of "::"; the crate name will be right before that.
        let end = demangled_full_symbol.get(beg..)
            .and_then(|s| s.find("::"))
            .map(|end_idx| beg + end_idx);

        if let Some(end) = end {
            // If the above search for "::" passed an " as " substring, 
            // we skip it and let the next iteration of the loop handle it to avoid doubly counting it.
            if demangled_full_symbol.get(beg..end).map(|s| s.contains(AS)) != Some(true) {
                // Find the beginning of the crate name, searching backwards from the "end"
                let start = demangled_full_symbol.get(beg..end)
                    .and_then(|s| s.rfind(|c| !is_valid_crate_name_char(c))) // find the first char before the crate name that isn't part of the crate name
                    .map(|start_idx| beg + start_idx + 1) // move forward to the actual start of the crate name
                    .unwrap_or(beg); // the crate name might have started at the beginning of the substring (no preceding non-crate-name char)
        
                ranges.push(start..end);
            }
        }

        // Advance to the next part of the symbol string
        beginning_bound = demangled_full_symbol.get(beg..)
            .and_then(|s| s.find(AS))
            .map(|beg_idx| beg + beg_idx + AS.len());
    }

    ranges
}


/// Replaces the `old_crate_name` substring in the given `demangled_full_symbol` with the given `new_crate_name`, 
/// if it can be found, and if the parent crate name matches the `old_crate_name`. 
/// If the parent crate name can be found but does not match the expected `old_crate_name`,
/// then None is returned.
/// 
/// This creates an entirely new String rather than performing an in-place replacement, 
/// because the `new_crate_name` might be a different length than the original crate name.
/// 
/// We cannot simply use `str::replace()` because it would replace *all* instances of the `old_crate_name`, 
/// including components of function/symbol names. 
/// 
/// # Examples
/// * `replace_containing_crate_name("keyboard::init", "keyboard", "keyboard_new")  ->  Some("keyboard_new::init")`
/// * `replace_containing_crate_name("<framebuffer::VirtualFramebuffer as display::Display>::fill_rectangle", "display", "display3")
///    ->  Some("<framebuffer::VirtualFramebuffer as display3::Display>::fill_rectangle")`
pub fn replace_containing_crate_name(demangled_full_symbol: &str, old_crate_name: &str, new_crate_name: &str) -> Option<String> {
    let mut new_symbol = String::from(demangled_full_symbol);
    let mut addend: isize = 0; // index compensation for the difference in old vs. new crate name length
    let mut replaced = false;
    for range in get_containing_crate_name_ranges(demangled_full_symbol) {
        if demangled_full_symbol.get(range.clone()) == Some(old_crate_name) {
            new_symbol = format!("{}{}{}", 
                &new_symbol[.. ((range.start as isize + addend) as usize)],
                new_crate_name,
                &demangled_full_symbol[range.end ..]
            );
            replaced = true;
            addend += (new_crate_name.len() as isize) - (old_crate_name.len() as isize);
        }
    }
    if replaced { Some(new_symbol) } else { None }
}
