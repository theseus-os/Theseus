//! Principles and Guidelines for Theseus Development and Contribution. 
//! 
//! # The Golden Rule of Software Development
//! 
//! *Code for others how you wish they would code for you.*
//! 
//! What does this mean? You should adhere to the following principles. 
//! 
//! * **Good abstractions.** Another developer using your code should never have to study the internals of the code itself,
//!   but rather be able to fully understand how to use your code simply from its struct/function names and documentation.
//!   Use intuitive names and try to design an interface that makes sense, is simple, easy to use, and doesn't surprise anyone with unnecessary trickery. 
//! 
//! * **Be clean**. Write well-crafted, concise code with sufficient features to be useful, but without bloat.
//!   Adhere to code style conventions, including proper spacing, doc comments, naming conventions, etc.
//! 
//! * **Foolproof code**. Think carefully about how others will use your code, 
//!   and design it thoughtfully to prevent others from making mistakes when using your code,
//!   ideally prevented at compile time instead of runtime. 
//! 
//! * **Errors are important!**  Handle errors gracefully and thoroughly, 
//!   and return detailed error messages that clearly describe the issue. *Don't ever let something fail silently!*
//! 
//! 
//! # Rust-specific or Theseus-specific Guidelines
//! 
//! * **Rust documentation.** Use proper rustdoc-style documentation *for all structs, functions, and types.* 
//!   Make sure all of your documentation links are correct, and that you're using the correct rustdoc formatting for doc comments. 
//!   Triple slashes `///` should be used above function and struct definitions, double slashes `//` for C-style inline comments (or block comments like `/* */`), and `//! ` for crate top-level documentation. 
//!   Use Markdown formatting to describe function arguments, return values, and include usage examples, in a way consistent with Rust's official libraries. 
//! 
//! * **Accurate metadata.**  In addition to good code and documentation, make sure to fill in additional metadata,
//!   such as the details present in each crate's `Cargo.toml` file: description, keywords, authors, etc.
//! 
//! * **`Option`s and `Result`s.** Use Options and Results properly. Don't use special values that have overloaded meanings, e.g., an integer in which `0` means no value, or something like that.
//!   [Here's a good resource](<https://blog.burntsushi.net/rust-error-handling/>) for better understanding error handling in Rust.
//! 
//!   `Option`s should be returned when an operation might fail, but that failure condition doesn't affect the rest of the system. 
//!   For example, if you're searching for an element in a list, then an `Option` is the suitable choice because the caller of your getter function would only call it in order to get and use the return value. 
//!   
//!   `Result`s should be returned if something can fail or succeed, and the caller needs to know whether it succeeded, but potentially need the actual return value, e.g., an init function that returns void. 
//!   In this case, `Result` is the best choice because we want to force the caller to acknowledge that the init function succeeded, or handle its error if it failed. 
//!   In Theseus, `Results` are mandatory when a function has some side effect, such as setting a parameter or value that might not exist or be initialized yet. 
//!   In that case, a result must be used to indicate whether the function succeeded. 
//!   
//!   **Handle `Result`s properly and fully.** Don't ignore a result error, instead, log that error and then handle it if possible. 
//!   If you cannot handle it, return that error to the caller so they can attempt to handle it. **NEVER SILENCE OR IGNORE ERRORS**.
//! 
//! * **Rust style**. Follow proper Rust coding style and naming conventions. Use correct spacing, indentation, and alignment that matches the existing style. 
//!   Make your code visually appealing, with spaces between operators like equal signs, addition, spaces after a comma, etc. Punctuation is important for legibility!
//! 
//! * **Never use unsafe code.** If you absolutely cannot avoid it, then you should review your case on an individual basis with the maintainer. 
//!   In 99.99% of cases, unsafe code is not necessary and can be rewritten safely. 
//! 
//! * **Never use panics.**  Avoid code or functions that can panic, such as bracket indexing operations `[]`, or panicking functions like `unwrap()` or `expect()`. 
//!   Instead, handle these error cases explicitly and return a `Result` to the caller, which is much cleaner than panicking. 
//!   Panicking is dangerous and cannot be easily recovered from.
//! 
//! * **No "magic" numbers.** Do not use literal number values that have no documentation or explanation of why they exist. 
//!   For example, instead of just writing a value like 4096 in the code, create a `const` that accurately describes the semantic meaning of that value, e.g., `const PAGE_SIZE: usize = 4096;`. 
//!   Magic numbers are terrible to maintain and don't help anyone who looks at your code in the future. 
//! 
//! * **Minimize global states.** Remove static (global) states as much as possible, and rethink how the same data sharing can be done without globals.
//! 
//! 
//! # Advice for Contributing and using git
//! 
//! * **Never push to the main branch.** Instead, checkout your own branch, develop your feature on that branch, 
//!   and then submit a pull request. This way, people can review your code, check for pitfalls and compatibility problems,
//!   and make comments and suggestions before the code makes its way into the main branch. 
//!   *You should do this for all changes, even tiny ones that may seem insignificant.*
//! 
//! * **Commit carefully.** When making a commit, review your changes with `git status` and `git diff`
//!   to ensure that you're not committing accidental modifications, or editing files that you shouldn't be.
//! 
//! * **Review yourself.** Perform an initial review of your own code before submitting a pull request. 
//!   Don't place the whole burden of fixing a bunch of tiny problems on others that must review your code too. 
//!   This includes building the documentation and reviewing it in HTML form in a browser 
//!   to make sure everything is formatted correctly and that hyperlinks work corretly. 
//! 
//! 
//! # Adding New Functionality to Theseus
//! 
//! The easiest way to add new functionality is just to create a new crate by duplicating an existing crate and changing the details in its new `Cargo.toml` file.
//! At the very least, you'll need to change the `name` entry under the `[package]` heading at the top of the `Cargo.toml` file, and you'll need to change the dependencies for your new crate.     
//!
//! If your new crate needs to be initialized, you can invoke it from the [`captain::init()`](../captain/fn.init.html) function, 
//! although there may be more appropriate places to do so, such as the [`driver_init::init()`](../driver_init/fn.init.html) function for drivers.
//! 
