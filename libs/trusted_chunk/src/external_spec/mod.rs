//! The external spec files are specifications of commonly used types like `Option`, `Vec` and `RangeInclusive`.
//! We also provide helper functions to peek into the values of the `Result` and `Option` types to be used in specifications.
//! The specs are simple and easy to understand, and only the subset of functions used in the verification are specified.
//! It is expected that type definitions given here are only used during verification, and the actual crates will be used when running the application.
//! For that, we use conditional compilation.

pub(crate) mod trusted_range_inclusive;
pub(crate) mod trusted_option;
pub(crate) mod trusted_result;