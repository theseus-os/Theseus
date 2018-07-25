//! Principles and Guidelines for Theseus design and development. 
//! 
//! # The Golden Rule of Software Development
//! *Code for others how you wish they would code for you.*
//! 
//! What does this mean? In a nutshell:
//! 
//! * **Good abstractions.** Another developer using your code should never have to study the code itself,
//!   but rather be able to fully understand how to use your code simply from its struct/function names and documentation.
//! * **Be clean**. Write well-crafted, concise code with sufficient features to be useful, but without bloat.
//!   Adhere to Rust's style conventions, including proper spacing, doc comments, naming conventions, etc.
//! * **Foolproof code**. Think carefully about how others will use your code, 
//!   and design it thoughtfully to prevent others from making mistakes when using your code,
//!   ideally prevented at compile time instead of runtime. 
//! * **Errors are important!**  Handle errors gracefully and thoroughly, 
//!   and return detailed error messages that clearly describe the issue.
//! * **Accurate metadata.**  In addition to good code and documentation, make sure to fill in additional metadata,
//!   such as the details present in each crate's `Cargo.toml` file: description, keywords, authors, etc.
//! 
//! 
