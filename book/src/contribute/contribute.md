# The Golden Rule of Software Development

*Code for others how you wish they would code for you.*

What does this mean? You should adhere to the following principles.

* **Good abstractions.** Another developer using your code should never have to study the internals of the code itself,
  but rather be able to fully understand how to use your code simply from its struct/function names and documentation.
  Use intuitive names and try to design an interface that makes sense, is simple and easy to use, and doesn't surprise anyone with unnecessary trickery.

* **Be clean.** Write well-crafted, concise code with sufficient features to be useful, but without bloat.
  Adhere to code style conventions, including proper spacing, doc comments, naming conventions, etc.

* **Foolproof code.** Think carefully about how others will use your code,
  and design it thoughtfully to prevent others from making mistakes when using your code,
  ideally prevented at compile time instead of runtime.

* **Errors are important!**  Handle errors gracefully and thoroughly,
  and return detailed error messages that clearly describe the issue. *Don't ever let something fail silently!*

Below are some other good practices.

* **Accurate metadata.**  In addition to good code and documentation, make sure to fill in additional metadata,
  such as the details present in each crate's `Cargo.toml` file: description, keywords, authors, etc.

* **No "magic" numbers.** Do not use literal number values that have no documentation or explanation of why they exist.
  For example, instead of just writing a value like 4096 in the code, create a `const` that accurately describes the semantic meaning of that value, e.g., `const PAGE_SIZE: usize = 4096;`.
  Magic numbers are terrible to maintain and don't help anyone who looks at your code in the future.

* **Minimize global states.** Remove static (global) states as much as possible, and rethink how the same data sharing can be done without globals.

## Rust-specific Guidelines

* **Rust style.** Follow proper Rust coding style and naming conventions. Use correct spacing, indentation, and alignment that matches the existing style.
  Make your code visually appealing, with spaces between operators like equal signs, addition, spaces after a comma, etc. Punctuation is important for legibility!

* **Rust documentation.** Use proper rustdoc-style documentation *for all structs, functions, and types.*
  Make sure all of your documentation links are correct, and that you're using the correct rustdoc formatting for doc comments.
  Triple slashes `///` should be used above function and struct definitions, double slashes `//` for C-style inline comments (or block comments like `/* */`), and `//! ` for crate top-level documentation.
  Use Markdown formatting to describe function arguments, return values, and include usage examples, in a way consistent with Rust's official libraries.

* **`Option`s and `Result`s.** Use Options and Results properly. Don't use special values that have overloaded meanings, e.g., an integer in which `0` means no value, or something like that.
  [Here's a good resource](<https://blog.burntsushi.net/rust-error-handling/>) for better understanding error handling in Rust.

  `Option`s should be returned when an operation might fail, but that failure condition doesn't affect the rest of the system.
  For example, if you're searching for an element in a list, then an `Option` is the suitable choice because the caller of your getter function would only call it in order to get and use the return value.

  `Result`s should be returned if something can fail or succeed, and the caller needs to know whether it succeeded, but potentially need the actual return value, e.g., an init function that returns void.
  In this case, `Result` is the best choice because we want to force the caller to acknowledge that the init function succeeded, or handle its error if it failed.
  In Theseus, `Results` are mandatory when a function has some side effect, such as setting a parameter or value that might not exist or be initialized yet.
  In that case, a result must be used to indicate whether the function succeeded.


## Theseus-specific Guidelines

* **Handle `Result`s properly and fully.** Don't ignore a result error, instead, log that error and then handle it if possible.
  If you cannot handle it, return that error to the caller so they can attempt to handle it. **NEVER SILENCE OR LAZILY HIDE ERRORS**.


* **Never use unsafe code.** If you absolutely cannot avoid it, then you should review your case on an individual basis with the maintainers of Theseus. In most cases, unsafe code is not necessary and can be rewritten in safe code.



## Adding New Functionality to Theseus

The easiest way to add new functionality is just to create a new crate by duplicating an existing crate and changing the details in its new `Cargo.toml` file.
At the very least, you'll need to change the `name` entry under the `[package]` heading at the top of the `Cargo.toml` file, and you'll need to change the dependencies for your new crate.

If your new kernel crate needs to be initialized, you can invoke it from the [`captain::init()` function](https://theseus-os.github.io/Theseus/doc/captain/index.html),
although there may be more appropriate places to do so, such as the [`device_manager`'s functions](https://theseus-os.github.io/Theseus/doc/device_manager/index.html) for initializing device drivers.

If you want to create a new application for Theseus, see [those instructions here](../app/app.md).
