//! stdlib implementation for Redox, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/stdlib.h.html

use core::{convert::TryFrom, intrinsics, iter, mem, ptr, slice};
use rand::{
    distributions::{Alphanumeric, Distribution, Uniform},
    prng::XorShiftRng,
    rngs::JitterRng,
    Rng, SeedableRng,
};

// use crate::{
//     c_str::CStr,
//     fs::File,
//     header::{
//         ctype,
//         errno::{self, *},
//         fcntl::*,
//         limits,
//         string::*,
//         time::{constants::CLOCK_MONOTONIC, timespec},
//         unistd::{self, sysconf, _SC_PAGESIZE},
//         wchar::*,
//     },
//     platform::{self, types::*, Pal, Sys},
// };

use crate::{
    types::*,
    errno::*,
    string::*,
    ctype,
};

pub const EXIT_FAILURE: c_int = 1;
pub const EXIT_SUCCESS: c_int = 0;
pub const RAND_MAX: c_int = 2_147_483_647;

//Maximum number of bytes in a multibyte character for the current locale
pub const MB_CUR_MAX: c_int = 4;
//Maximum number of bytes in a multibyte characters for any locale
pub const MB_LEN_MAX: c_int = 4;

static mut ATEXIT_FUNCS: [Option<extern "C" fn()>; 32] = [None; 32];
static mut L64A_BUFFER: [c_char; 7] = [0; 7]; // up to 6 digits plus null terminator
static mut RNG: Option<XorShiftRng> = None;

lazy_static! {
    static ref RNG_SAMPLER: Uniform<c_int> = Uniform::new_inclusive(0, RAND_MAX);
}

// #[no_mangle]
// pub extern "C" fn _Exit(status: c_int) {
//     unistd::_exit(status);
// }

// #[no_mangle]
// pub unsafe extern "C" fn a64l(s: *const c_char) -> c_long {
//     if s.is_null() {
//         return 0;
//     }
//     // POSIX says only the low-order 32 bits are used.
//     let mut l: i32 = 0;
//     // a64l does not support more than 6 characters at once
//     for x in 0..6 {
//         let c = *s.offset(x);
//         if c == 0 {
//             // string is null terminated
//             return c_long::from(l);
//         }
//         // ASCII to base64 conversion:
//         let mut bits: i32 = if c < 58 {
//             (c - 46) as i32 // ./0123456789
//         } else if c < 91 {
//             (c - 53) as i32 // A-Z
//         } else {
//             (c - 59) as i32 // a-z
//         };
//         bits <<= 6 * x;
//         l |= bits;
//     }
//     c_long::from(l)
// }

// #[no_mangle]
// pub unsafe extern "C" fn abort() {
//     intrinsics::abort();
// }

// #[no_mangle]
// pub extern "C" fn abs(i: c_int) -> c_int {
//     i.abs()
// }

// #[no_mangle]
// pub unsafe extern "C" fn aligned_alloc(alignment: size_t, size: size_t) -> *mut c_void {
//     if size % alignment == 0 {
//         /* The size-is-multiple-of-alignment requirement is the only
//          * difference between aligned_alloc() and memalign(). */
//         memalign(alignment, size)
//     } else {
//         platform::errno = EINVAL;
//         ptr::null_mut()
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn atexit(func: Option<extern "C" fn()>) -> c_int {
//     for i in 0..ATEXIT_FUNCS.len() {
//         if ATEXIT_FUNCS[i] == None {
//             ATEXIT_FUNCS[i] = func;
//             return 0;
//         }
//     }

//     1
// }

// #[no_mangle]
// pub unsafe extern "C" fn atof(s: *const c_char) -> c_double {
//     strtod(s, ptr::null_mut())
// }

macro_rules! dec_num_from_ascii {
    ($s:expr, $t:ty) => {
        unsafe {
            let mut s = $s;
            // Iterate past whitespace
            while ctype::isspace(*s as c_int) != 0 {
                s = s.offset(1);
            }

            // Find out if there is a - sign
            let neg_sign = match *s {
                0x2d => {
                    s = s.offset(1);
                    true
                }
                // '+' increment s and continue parsing
                0x2b => {
                    s = s.offset(1);
                    false
                }
                _ => false,
            };

            let mut n: $t = 0;
            while ctype::isdigit(*s as c_int) != 0 {
                n = 10 * n - (*s as $t - 0x30);
                s = s.offset(1);
            }

            if neg_sign {
                n
            } else {
                -n
            }
        }
    };
}

#[no_mangle]
pub extern "C" fn atoi(s: *const c_char) -> c_int {
    dec_num_from_ascii!(s, c_int)
}

#[no_mangle]
pub extern "C" fn atol(s: *const c_char) -> c_long {
    dec_num_from_ascii!(s, c_long)
}

// #[no_mangle]
// pub extern "C" fn atoll(s: *const c_char) -> c_longlong {
//     c!(s, c_longlong)
// }

unsafe extern "C" fn void_cmp(a: *const c_void, b: *const c_void) -> c_int {
    *(a as *const i32) - *(b as *const i32) as c_int
}

// #[no_mangle]
// pub unsafe extern "C" fn bsearch(
//     key: *const c_void,
//     base: *const c_void,
//     nel: size_t,
//     width: size_t,
//     compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
// ) -> *mut c_void {
//     let mut start = base;
//     let mut len = nel;
//     let cmp_fn = compar.unwrap_or(void_cmp);
//     while len > 0 {
//         let med = (start as size_t + (len >> 1) * width) as *const c_void;
//         let diff = cmp_fn(key, med);
//         if diff == 0 {
//             return med as *mut c_void;
//         } else if diff > 0 {
//             start = (med as usize + width) as *const c_void;
//             len -= 1;
//         }
//         len >>= 1;
//     }
//     ptr::null_mut()
// }

// #[no_mangle]
// pub unsafe extern "C" fn calloc(nelem: size_t, elsize: size_t) -> *mut c_void {
//     //Handle possible integer overflow in size calculation
//     match nelem.checked_mul(elsize) {
//         Some(size) => {
//             /* If allocation fails here, errno setting will be handled
//              * by malloc() */
//             let ptr = malloc(size);
//             if !ptr.is_null() {
//                 intrinsics::write_bytes(ptr as *mut u8, 0, size);
//             }
//             ptr
//         }
//         None => {
//             // For overflowing multiplication, we have to set errno here
//             platform::errno = ENOMEM;
//             ptr::null_mut()
//         }
//     }
// }

// #[repr(C)]
// pub struct div_t {
//     quot: c_int,
//     rem: c_int,
// }

// #[no_mangle]
// pub extern "C" fn div(numer: c_int, denom: c_int) -> div_t {
//     div_t {
//         quot: numer / denom,
//         rem: numer % denom,
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn drand48() -> c_double {
//     let new_xi = lcg48::generator_step(lcg48::DEFAULT_XI.as_mut_ptr());
//     lcg48::float64_from_x(new_xi)
// }

// // #[no_mangle]
// pub extern "C" fn ecvt(
//     value: c_double,
//     ndigit: c_int,
//     decpt: *mut c_int,
//     sign: *mut c_int,
// ) -> *mut c_char {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn erand48(xsubi: *mut c_ushort) -> c_double {
//     let new_xi = lcg48::generator_step(xsubi);
//     lcg48::float64_from_x(new_xi)
// }

// #[no_mangle]
// pub unsafe extern "C" fn exit(status: c_int) {
//     extern "C" {
//         static __fini_array_start: extern "C" fn();
//         static __fini_array_end: extern "C" fn();

//         fn pthread_terminate();
//         fn _fini();
//     }

//     for i in (0..ATEXIT_FUNCS.len()).rev() {
//         if let Some(func) = ATEXIT_FUNCS[i] {
//             (func)();
//         }
//     }

//     // Look for the neighbor functions in memory until the end
//     let mut f = &__fini_array_end as *const _;
//     #[allow(clippy::op_ref)]
//     while f > &__fini_array_start {
//         f = f.offset(-1);
//         (*f)();
//     }

//     _fini();

//     pthread_terminate();

//     Sys::exit(status);
// }

// // #[no_mangle]
// pub extern "C" fn fcvt(
//     value: c_double,
//     ndigit: c_int,
//     decpt: *mut c_int,
//     sign: *mut c_int,
// ) -> *mut c_char {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn free(ptr: *mut c_void) {
//     platform::free(ptr);
// }

// // #[no_mangle]
// pub extern "C" fn gcvt(value: c_double, ndigit: c_int, buf: *mut c_char) -> *mut c_char {
//     unimplemented!();
// }

// unsafe fn find_env(search: *const c_char) -> Option<(usize, *mut c_char)> {
//     for (i, item) in platform::inner_environ.iter().enumerate() {
//         let mut item = *item;
//         if item.is_null() {
//             assert_eq!(
//                 i,
//                 platform::inner_environ.len() - 1,
//                 "an early null pointer in environ vector"
//             );
//             break;
//         }
//         let mut search = search;
//         loop {
//             let end_of_query = *search == 0 || *search == b'=' as c_char;
//             assert_ne!(*item, 0, "environ has an item without value");
//             if *item == b'=' as c_char || end_of_query {
//                 if *item == b'=' as c_char && end_of_query {
//                     // Both keys env here
//                     return Some((i, item.offset(1)));
//                 } else {
//                     break;
//                 }
//             }

//             if *item != *search {
//                 break;
//             }

//             item = item.offset(1);
//             search = search.offset(1);
//         }
//     }
//     None
// }

// #[no_mangle]
// pub unsafe extern "C" fn getenv(name: *const c_char) -> *mut c_char {
//     find_env(name).map(|val| val.1).unwrap_or(ptr::null_mut())
// }

// // #[no_mangle]
// pub extern "C" fn getsubopt(
//     optionp: *mut *mut c_char,
//     tokens: *const *mut c_char,
//     valuep: *mut *mut c_char,
// ) -> c_int {
//     unimplemented!();
// }

// // #[no_mangle]
// pub extern "C" fn grantpt(fildes: c_int) -> c_int {
//     unimplemented!();
// }

// // #[no_mangle]
// pub extern "C" fn initstate(seec: c_uint, state: *mut c_char, size: size_t) -> *mut c_char {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn jrand48(xsubi: *mut c_ushort) -> c_long {
//     let new_xi = lcg48::generator_step(xsubi);
//     lcg48::int32_from_x(new_xi)
// }

// #[no_mangle]
// pub unsafe extern "C" fn l64a(value: c_long) -> *mut c_char {
//     // POSIX says we should only consider the lower 32 bits of value.
//     let value_as_i32 = value as i32;

//     /* If we pretend to extend the 32-bit value with 4 binary zeros, we
//      * would get a 36-bit integer. The number of base-64 digits to be
//      * left unused can then be found by taking the number of leading
//      * zeros, dividing by 6 and rounding down (i.e. using integer
//      * division). */
//     let num_output_digits = 6 - (value_as_i32.leading_zeros() + 4) / 6;

//     // Reset buffer (and have null terminator in place for any result)
//     L64A_BUFFER = [0; 7];

//     for i in 0..num_output_digits as usize {
//         let digit_value = ((value_as_i32 >> 6 * i) & 63) as c_char;

//         if digit_value < 12 {
//             // ./0123456789 for values 0 to 11. b'.' == 46
//             L64A_BUFFER[i] = 46 + digit_value;
//         } else if digit_value < 38 {
//             // A-Z for values 12 to 37. b'A' == 65, 65-12 == 53
//             L64A_BUFFER[i] = 53 + digit_value;
//         } else {
//             // a-z for values 38 to 63. b'a' == 97, 97-38 == 59
//             L64A_BUFFER[i] = 59 + digit_value;
//         }
//     }

//     L64A_BUFFER.as_mut_ptr()
// }

// #[no_mangle]
// pub extern "C" fn labs(i: c_long) -> c_long {
//     i.abs()
// }

// #[no_mangle]
// pub unsafe extern "C" fn lcong48(param: *mut c_ushort) {
//     // Input should be a size-7 array.

//     /* Go through this ptr -> u64 -> ptr conversion to ensure we only
//      * get the lower 16 bits of each element. */
//     let new_xi = lcg48::uint48_from_ushort_arr3(param.offset(0));
//     lcg48::set_ushort_arr3_from_uint48(lcg48::DEFAULT_XI.as_mut_ptr(), new_xi);
//     lcg48::A = lcg48::uint48_from_ushort_arr3(param.offset(3));
//     lcg48::C = *param.offset(6) as u16; // c_ushort may be more than 16 bits
// }

// #[repr(C)]
// pub struct ldiv_t {
//     quot: c_long,
//     rem: c_long,
// }

// #[no_mangle]
// pub extern "C" fn ldiv(numer: c_long, denom: c_long) -> ldiv_t {
//     ldiv_t {
//         quot: numer / denom,
//         rem: numer % denom,
//     }
// }

// #[no_mangle]
// pub extern "C" fn llabs(i: c_longlong) -> c_longlong {
//     i.abs()
// }

// #[repr(C)]
// pub struct lldiv_t {
//     quot: c_longlong,
//     rem: c_longlong,
// }

// #[no_mangle]
// pub extern "C" fn lldiv(numer: c_longlong, denom: c_longlong) -> lldiv_t {
//     lldiv_t {
//         quot: numer / denom,
//         rem: numer % denom,
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn lrand48() -> c_long {
//     let new_xi = lcg48::generator_step(lcg48::DEFAULT_XI.as_mut_ptr());
//     lcg48::uint31_from_x(new_xi)
// }

// #[no_mangle]
// pub unsafe extern "C" fn malloc(size: size_t) -> *mut c_void {
//     let ptr = platform::alloc(size);
//     if ptr.is_null() {
//         platform::errno = ENOMEM;
//     }
//     ptr
// }

// #[no_mangle]
// pub unsafe extern "C" fn memalign(alignment: size_t, size: size_t) -> *mut c_void {
//     if alignment.is_power_of_two() {
//         let ptr = platform::alloc_align(size, alignment);
//         if ptr.is_null() {
//             platform::errno = ENOMEM;
//         }
//         ptr
//     } else {
//         platform::errno = EINVAL;
//         ptr::null_mut()
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn mblen(s: *const c_char, n: size_t) -> c_int {
//     let mut wc: wchar_t = 0;
//     let mut state: mbstate_t = mbstate_t {};
//     let result: usize = mbrtowc(&mut wc, s, n, &mut state);

//     if result == -1isize as usize {
//         return -1;
//     }
//     if result == -2isize as usize {
//         return -1;
//     }

//     result as i32
// }

// #[no_mangle]
// pub unsafe extern "C" fn mbstowcs(pwcs: *mut wchar_t, mut s: *const c_char, n: size_t) -> size_t {
//     let mut state: mbstate_t = mbstate_t {};
//     mbsrtowcs(pwcs, &mut s, n, &mut state)
// }

// #[no_mangle]
// pub unsafe extern "C" fn mbtowc(pwc: *mut wchar_t, s: *const c_char, n: size_t) -> c_int {
//     let mut state: mbstate_t = mbstate_t {};
//     mbrtowc(pwc, s, n, &mut state) as c_int
// }

// fn inner_mktemp<T, F>(name: *mut c_char, suffix_len: c_int, mut attempt: F) -> Option<T>
// where
//     F: FnMut() -> Option<T>,
// {
//     let len = unsafe { strlen(name) as c_int };

//     if len < 6 || suffix_len > len - 6 {
//         unsafe { platform::errno = errno::EINVAL };
//         return None;
//     }

//     for i in (len - suffix_len - 6)..(len - suffix_len) {
//         if unsafe { *name.offset(i as isize) } != b'X' as c_char {
//             unsafe { platform::errno = errno::EINVAL };
//             return None;
//         }
//     }

//     let mut rng = JitterRng::new_with_timer(get_nstime);
//     let _ = rng.test_timer();

//     for _ in 0..100 {
//         let char_iter = iter::repeat(())
//             .map(|()| rng.sample(Alphanumeric))
//             .take(6)
//             .enumerate();
//         unsafe {
//             for (i, c) in char_iter {
//                 *name.offset((len as isize) - (suffix_len as isize) - (i as isize) - 1) =
//                     c as c_char
//             }
//         }

//         if let result @ Some(_) = attempt() {
//             return result;
//         }
//     }

//     unsafe { platform::errno = errno::EEXIST }

//     None
// }

// #[no_mangle]
// pub unsafe extern "C" fn mktemp(name: *mut c_char) -> *mut c_char {
//     if inner_mktemp(name, 0, || {
//         let name = CStr::from_ptr(name);
//         if Sys::access(name, 0) != 0 && platform::errno == ENOENT {
//             Some(())
//         } else {
//             None
//         }
//     })
//     .is_none()
//     {
//         *name = 0;
//     }
//     name
// }

// fn get_nstime() -> u64 {
//     let mut ts: timespec = unsafe { mem::uninitialized() };
//     Sys::clock_gettime(CLOCK_MONOTONIC, &mut ts);
//     ts.tv_nsec as u64
// }

// #[no_mangle]
// pub extern "C" fn mkostemps(name: *mut c_char, suffix_len: c_int, mut flags: c_int) -> c_int {
//     flags &= !O_ACCMODE;
//     flags |= O_RDWR | O_CREAT | O_EXCL;

//     inner_mktemp(name, suffix_len, || {
//         let name = unsafe { CStr::from_ptr(name) };
//         let fd = Sys::open(name, flags, 0o600);

//         if fd >= 0 {
//             Some(fd)
//         } else {
//             None
//         }
//     })
//     .unwrap_or(-1)
// }

// #[no_mangle]
// pub extern "C" fn mkstemp(name: *mut c_char) -> c_int {
//     mkostemps(name, 0, 0)
// }
// #[no_mangle]
// pub extern "C" fn mkostemp(name: *mut c_char, flags: c_int) -> c_int {
//     mkostemps(name, 0, flags)
// }
// #[no_mangle]
// pub extern "C" fn mkstemps(name: *mut c_char, suffix_len: c_int) -> c_int {
//     mkostemps(name, suffix_len, 0)
// }

// #[no_mangle]
// pub unsafe extern "C" fn mrand48() -> c_long {
//     let new_xi = lcg48::generator_step(lcg48::DEFAULT_XI.as_mut_ptr());
//     lcg48::int32_from_x(new_xi)
// }

// #[no_mangle]
// pub unsafe extern "C" fn nrand48(xsubi: *mut c_ushort) -> c_long {
//     let new_xi = lcg48::generator_step(xsubi);
//     lcg48::uint31_from_x(new_xi)
// }

// #[no_mangle]
// pub unsafe extern "C" fn posix_memalign(
//     memptr: *mut *mut c_void,
//     alignment: size_t,
//     size: size_t,
// ) -> c_int {
//     const void_ptr_size: usize = mem::size_of::<*mut c_void>();

//     /* Check that alignment is:
//      * a) a multiple of sizeof(void *)
//      * b) a power-of-two multiple of sizeof(void *). Integer division as
//      *    below gives the correct result once a) is ensured. */
//     if alignment % void_ptr_size == 0 && (alignment / void_ptr_size).is_power_of_two() {
//         let ptr = platform::alloc_align(size, alignment);
//         if !ptr.is_null() {
//             *memptr = ptr;
//             0
//         } else {
//             ENOMEM
//         }
//     } else {
//         EINVAL
//     }
// }

// // #[no_mangle]
// pub extern "C" fn ptsname(fildes: c_int) -> *mut c_char {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn putenv(insert: *mut c_char) -> c_int {
//     assert_ne!(insert, ptr::null_mut(), "putenv(NULL)");
//     if let Some((i, _)) = find_env(insert) {
//         //TODO: find out why this crashes: platform::free(platform::inner_environ[i] as *mut c_void);
//         platform::inner_environ[i] = insert;
//     } else {
//         let i = platform::inner_environ.len() - 1;
//         assert_eq!(
//             platform::inner_environ[i],
//             ptr::null_mut(),
//             "environ did not end with null"
//         );
//         platform::inner_environ[i] = insert;
//         platform::inner_environ.push(ptr::null_mut());
//         platform::environ = platform::inner_environ.as_mut_ptr();
//     }
//     0
// }

// #[no_mangle]
// pub extern "C" fn qsort(
//     base: *mut c_void,
//     nel: size_t,
//     width: size_t,
//     compar: Option<extern "C" fn(*const c_void, *const c_void) -> c_int>,
// ) {
//     if let Some(comp) = compar {
//         // XXX: check width too?  not specified
//         if nel > 0 {
//             // XXX: maybe try to do mergesort/timsort first and fallback to introsort if memory
//             //      allocation fails?  not sure what is ideal
//             sort::introsort(base as *mut c_char, nel, width, comp);
//         }
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn rand() -> c_int {
//     match RNG {
//         Some(ref mut rng) => RNG_SAMPLER.sample(rng),
//         None => {
//             let mut rng = XorShiftRng::from_seed([1; 16]);
//             let ret = RNG_SAMPLER.sample(&mut rng);
//             RNG = Some(rng);
//             ret
//         }
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn rand_r(seed: *mut c_uint) -> c_int {
//     if seed.is_null() {
//         errno::EINVAL
//     } else {
//         // set the type explicitly so this will fail if the array size for XorShiftRng changes
//         let seed_arr: [u8; 16] = mem::transmute([*seed; 16 / mem::size_of::<c_uint>()]);

//         let mut rng = XorShiftRng::from_seed(seed_arr);
//         let ret = RNG_SAMPLER.sample(&mut rng);

//         *seed = ret as _;

//         ret
//     }
// }

// // #[no_mangle]
// pub extern "C" fn random() -> c_long {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: size_t) -> *mut c_void {
//     let new_ptr = platform::realloc(ptr, size);
//     if new_ptr.is_null() {
//         platform::errno = ENOMEM;
//     }
//     new_ptr
// }

// #[no_mangle]
// pub unsafe extern "C" fn realpath(pathname: *const c_char, resolved: *mut c_char) -> *mut c_char {
//     let ptr = if resolved.is_null() {
//         malloc(limits::PATH_MAX) as *mut c_char
//     } else {
//         resolved
//     };

//     let out = slice::from_raw_parts_mut(ptr as *mut u8, limits::PATH_MAX);
//     {
//         let file = match File::open(&CStr::from_ptr(pathname), O_PATH | O_CLOEXEC) {
//             Ok(file) => file,
//             Err(_) => return ptr::null_mut(),
//         };

//         let len = out.len();
//         let read = Sys::fpath(*file, &mut out[..len - 1]);
//         if read < 0 {
//             return ptr::null_mut();
//         }
//         out[read as usize] = 0;
//     }

//     ptr
// }

// #[no_mangle]
// pub unsafe extern "C" fn seed48(seed16v: *mut c_ushort) -> *mut c_ushort {
//     lcg48::reset_a_and_c();

//     lcg48::STASHED_XI = lcg48::DEFAULT_XI;

//     let new_xi = lcg48::uint48_from_ushort_arr3(seed16v);
//     lcg48::set_ushort_arr3_from_uint48(lcg48::DEFAULT_XI.as_mut_ptr(), new_xi);

//     lcg48::STASHED_XI.as_mut_ptr()
// }

// #[no_mangle]
// pub unsafe extern "C" fn setenv(
//     mut key: *const c_char,
//     mut value: *const c_char,
//     overwrite: c_int,
// ) -> c_int {
//     let mut key_len = 0;
//     while *key.offset(key_len) != 0 {
//         key_len += 1;
//     }
//     let mut value_len = 0;
//     while *value.offset(value_len) != 0 {
//         value_len += 1;
//     }

//     let index = if let Some((i, existing)) = find_env(key) {
//         if overwrite == 0 {
//             return 0;
//         }

//         let mut existing_len = 0;
//         while *existing.offset(existing_len) != 0 {
//             existing_len += 1;
//         }

//         if existing_len >= value_len {
//             // Reuse existing element's allocation
//             for i in 0..=value_len {
//                 *existing.offset(i) = *value.offset(i);
//             }
//             return 0;
//         } else {
//             i
//         }
//     } else {
//         let i = platform::inner_environ.len() - 1;
//         assert_eq!(
//             platform::inner_environ[i],
//             ptr::null_mut(),
//             "environ did not end with null"
//         );
//         platform::inner_environ.push(ptr::null_mut());
//         platform::environ = platform::inner_environ.as_mut_ptr();
//         i
//     };

//     //platform::free(platform::inner_environ[index] as *mut c_void);
//     let mut ptr = platform::alloc(key_len as usize + 1 + value_len as usize) as *mut c_char;
//     platform::inner_environ[index] = ptr;

//     while *key != 0 {
//         *ptr = *key;
//         ptr = ptr.offset(1);
//         key = key.offset(1);
//     }
//     *ptr = b'=' as c_char;
//     ptr = ptr.offset(1);
//     while *value != 0 {
//         *ptr = *value;
//         ptr = ptr.offset(1);
//         value = value.offset(1);
//     }
//     *ptr = 0;

//     0
// }

// // #[no_mangle]
// pub extern "C" fn setkey(key: *const c_char) {
//     unimplemented!();
// }

// // #[no_mangle]
// pub extern "C" fn setstate(state: *const c_char) -> *mut c_char {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn srand(seed: c_uint) {
//     RNG = Some(XorShiftRng::from_seed([seed as u8; 16]));
// }

// #[no_mangle]
// pub unsafe extern "C" fn srand48(seedval: c_long) {
//     lcg48::reset_a_and_c();

//     /* Set the high 32 bits of the 48-bit X_i value to the lower 32 bits
//      * of the input argument, and the lower 16 bits to 0x330e, as
//      * specified in POSIX. */
//     let new_xi = (((seedval as u32) as u64) << 16) | 0x330e_u64;
//     lcg48::set_ushort_arr3_from_uint48(lcg48::DEFAULT_XI.as_mut_ptr(), new_xi);
// }

// // #[no_mangle]
// pub extern "C" fn srandom(seed: c_uint) {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> c_double {
//     strto_float_impl!(c_double, s, endptr)
// }
// #[no_mangle]
// pub unsafe extern "C" fn strtof(s: *const c_char, endptr: *mut *mut c_char) -> c_float {
//     strto_float_impl!(c_float, s, endptr)
// }

// pub fn is_positive(ch: c_char) -> Option<(bool, isize)> {
//     match ch {
//         0 => None,
//         ch if ch == b'+' as c_char => Some((true, 1)),
//         ch if ch == b'-' as c_char => Some((false, 1)),
//         _ => Some((true, 0)),
//     }
// }

// pub unsafe fn detect_base(s: *const c_char) -> Option<(c_int, isize)> {
//     let first = *s as u8;
//     match first {
//         0 => None,
//         b'0' => {
//             let second = *s.offset(1) as u8;
//             if second == b'X' || second == b'x' {
//                 Some((16, 2))
//             } else if second >= b'0' && second <= b'7' {
//                 Some((8, 1))
//             } else {
//                 // in this case, the prefix (0) is going to be the number
//                 Some((8, 0))
//             }
//         }
//         _ => Some((10, 0)),
//     }
// }

// pub unsafe fn convert_octal(s: *const c_char) -> Option<(c_ulong, isize, bool)> {
//     if *s != 0 && *s == b'0' as c_char {
//         if let Some((val, idx, overflow)) = convert_integer(s.offset(1), 8) {
//             Some((val, idx + 1, overflow))
//         } else {
//             // in case the prefix is not actually a prefix
//             Some((0, 1, false))
//         }
//     } else {
//         None
//     }
// }

// pub unsafe fn convert_hex(s: *const c_char) -> Option<(c_ulong, isize, bool)> {
//     if (*s != 0 && *s == b'0' as c_char)
//         && (*s.offset(1) != 0 && (*s.offset(1) == b'x' as c_char || *s.offset(1) == b'X' as c_char))
//     {
//         convert_integer(s.offset(2), 16).map(|(val, idx, overflow)| (val, idx + 2, overflow))
//     } else {
//         convert_integer(s, 16).map(|(val, idx, overflow)| (val, idx, overflow))
//     }
// }

// pub unsafe fn convert_integer(s: *const c_char, base: c_int) -> Option<(c_ulong, isize, bool)> {
//     // -1 means the character is invalid
//     #[rustfmt::skip]
//     const LOOKUP_TABLE: [c_long; 256] = [
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//          0,  1,  2,  3,  4,  5,  6,  7,  8,  9, -1, -1, -1, -1, -1, -1,
//         -1, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
//         25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, -1, -1, -1, -1, -1,
//         -1, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
//         25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//         -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
//     ];

//     let mut num: c_ulong = 0;
//     let mut idx = 0;
//     let mut overflowed = false;

//     loop {
//         // `-1 as usize` is usize::MAX
//         // `-1 as u8 as usize` is u8::MAX
//         // It extends by the sign bit unless we cast it to unsigned first.
//         let val = LOOKUP_TABLE[*s.offset(idx) as u8 as usize];
//         if val == -1 || val as c_int >= base {
//             break;
//         } else {
//             if let Some(res) = num
//                 .checked_mul(base as c_ulong)
//                 .and_then(|num| num.checked_add(val as c_ulong))
//             {
//                 num = res;
//             } else {
//                 platform::errno = ERANGE;
//                 num = c_ulong::max_value();
//                 overflowed = true;
//             }

//             idx += 1;
//         }
//     }

//     if idx > 0 {
//         Some((num, idx, overflowed))
//     } else {
//         None
//     }
// }

// #[no_mangle]
// pub unsafe extern "C" fn strtoul(
//     s: *const c_char,
//     endptr: *mut *mut c_char,
//     base: c_int,
// ) -> c_ulong {
//     strto_impl!(
//         c_ulong,
//         false,
//         c_ulong::max_value(),
//         c_ulong::min_value(),
//         s,
//         endptr,
//         base
//     )
// }

// #[no_mangle]
// pub unsafe extern "C" fn strtol(s: *const c_char, endptr: *mut *mut c_char, base: c_int) -> c_long {
//     strto_impl!(
//         c_long,
//         true,
//         c_long::max_value(),
//         c_long::min_value(),
//         s,
//         endptr,
//         base
//     )
// }

// #[no_mangle]
// pub unsafe extern "C" fn strtoull(
//     s: *const c_char,
//     endptr: *mut *mut c_char,
//     base: c_int,
// ) -> c_ulonglong {
//     strtoul(s, endptr, base)
// }

// #[no_mangle]
// pub unsafe extern "C" fn strtoll(
//     s: *const c_char,
//     endptr: *mut *mut c_char,
//     base: c_int,
// ) -> c_longlong {
//     strtol(s, endptr, base)
// }

// #[no_mangle]
// pub unsafe extern "C" fn system(command: *const c_char) -> c_int {
//     //TODO: share code with popen

//     let child_pid = unistd::fork();
//     if child_pid == 0 {
//         let command_nonnull = if command.is_null() {
//             "exit 0\0".as_ptr()
//         } else {
//             command as *const u8
//         };

//         let shell = "/bin/sh\0".as_ptr();

//         let args = [
//             "sh\0".as_ptr(),
//             "-c\0".as_ptr(),
//             command_nonnull,
//             ptr::null(),
//         ];

//         unistd::execv(shell as *const c_char, args.as_ptr() as *const *mut c_char);

//         exit(127);

//         unreachable!();
//     } else if child_pid > 0 {
//         let mut wstatus = 0;
//         if Sys::waitpid(child_pid, &mut wstatus, 0) == !0 {
//             return -1;
//         }

//         wstatus
//     } else {
//         -1
//     }
// }

// // #[no_mangle]
// pub extern "C" fn ttyslot() -> c_int {
//     unimplemented!();
// }

// // #[no_mangle]
// pub extern "C" fn unlockpt(fildes: c_int) -> c_int {
//     unimplemented!();
// }

// #[no_mangle]
// pub unsafe extern "C" fn unsetenv(key: *const c_char) -> c_int {
//     if let Some((i, _)) = find_env(key) {
//         // No need to worry about updating the pointer, this does not
//         // reallocate in any way. And the final null is already shifted back.
//         platform::inner_environ.remove(i);
//     }
//     0
// }

// #[no_mangle]
// pub unsafe extern "C" fn valloc(size: size_t) -> *mut c_void {
//     /* sysconf(_SC_PAGESIZE) is a c_long and may in principle not
//      * convert correctly to a size_t. */
//     match size_t::try_from(sysconf(_SC_PAGESIZE)) {
//         Ok(page_size) => {
//             /* valloc() is not supposed to be able to set errno to
//              * EINVAL, hence no call to memalign(). */
//             let ptr = platform::alloc_align(size, page_size);
//             if ptr.is_null() {
//                 platform::errno = ENOMEM;
//             }
//             ptr
//         }
//         Err(_) => {
//             // A corner case. No errno setting.
//             ptr::null_mut()
//         }
//     }
// }

// #[no_mangle]
// pub extern "C" fn wcstombs(s: *mut c_char, pwcs: *mut *const wchar_t, n: size_t) -> size_t {
//     let mut state: mbstate_t = mbstate_t {};
//     wcsrtombs(s, pwcs, n, &mut state)
// }

// #[no_mangle]
// pub unsafe extern "C" fn wctomb(s: *mut c_char, wc: wchar_t) -> c_int {
//     let mut state: mbstate_t = mbstate_t {};
//     let result: usize = wcrtomb(s, wc, &mut state);

//     if result == -1isize as usize {
//         return -1;
//     }
//     if result == -2isize as usize {
//         return -1;
//     }

//     result as c_int
// }
