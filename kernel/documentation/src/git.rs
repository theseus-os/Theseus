
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