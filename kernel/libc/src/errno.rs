//! errno implementation for Redox, following http://pubs.opengroup.org/onlinepubs/7908799/xsh/errno.h.html

// use crate::platform::{self, types::*};

// //TODO: Consider removing, provided for compatibility with newlib
// #[no_mangle]
// pub unsafe extern "C" fn __errno() -> *mut c_int {
//     __errno_location()
// }

// #[no_mangle]
// pub unsafe extern "C" fn __errno_location() -> *mut c_int {
//     &mut platform::errno
// }

// #[no_mangle]
// pub unsafe extern "C" fn __program_invocation_name() -> *mut c_char {
//     platform::inner_argv[0]
// }


use crate::types::*;

// #[thread_local]
#[allow(non_upper_case_globals)]
#[no_mangle]
pub static mut errno: c_int = 0;

pub const EPERM: c_int = 1; /* Operation not permitted */
pub const ENOENT: c_int = 2; /* No such file or directory */
pub const ESRCH: c_int = 3; /* No such process */
pub const EINTR: c_int = 4; /* Interrupted system call */
pub const EIO: c_int = 5; /* I/O error */
pub const ENXIO: c_int = 6; /* No such device or address */
pub const E2BIG: c_int = 7; /* Argument list too long */
pub const ENOEXEC: c_int = 8; /* Exec format error */
pub const EBADF: c_int = 9; /* Bad file number */
pub const ECHILD: c_int = 10; /* No child processes */
pub const EAGAIN: c_int = 11; /* Try again */
pub const ENOMEM: c_int = 12; /* Out of memory */
pub const EACCES: c_int = 13; /* Permission denied */
pub const EFAULT: c_int = 14; /* Bad address */
pub const ENOTBLK: c_int = 15; /* Block device required */
pub const EBUSY: c_int = 16; /* Device or resource busy */
pub const EEXIST: c_int = 17; /* File exists */
pub const EXDEV: c_int = 18; /* Cross-device link */
pub const ENODEV: c_int = 19; /* No such device */
pub const ENOTDIR: c_int = 20; /* Not a directory */
pub const EISDIR: c_int = 21; /* Is a directory */
pub const EINVAL: c_int = 22; /* Invalid argument */
pub const ENFILE: c_int = 23; /* File table overflow */
pub const EMFILE: c_int = 24; /* Too many open files */
pub const ENOTTY: c_int = 25; /* Not a typewriter */
pub const ETXTBSY: c_int = 26; /* Text file busy */
pub const EFBIG: c_int = 27; /* File too large */
pub const ENOSPC: c_int = 28; /* No space left on device */
pub const ESPIPE: c_int = 29; /* Illegal seek */
pub const EROFS: c_int = 30; /* Read-only file system */
pub const EMLINK: c_int = 31; /* Too many links */
pub const EPIPE: c_int = 32; /* Broken pipe */
pub const EDOM: c_int = 33; /* Math argument out of domain of func */
pub const ERANGE: c_int = 34; /* Math result not representable */
pub const EDEADLK: c_int = 35; /* Resource deadlock would occur */
pub const ENAMETOOLONG: c_int = 36; /* File name too long */
pub const ENOLCK: c_int = 37; /* No record locks available */
pub const ENOSYS: c_int = 38; /* Function not implemented */
pub const ENOTEMPTY: c_int = 39; /* Directory not empty */
pub const ELOOP: c_int = 40; /* Too many symbolic links encountered */
pub const EWOULDBLOCK: c_int = 41; /* Operation would block */
pub const ENOMSG: c_int = 42; /* No message of desired type */
pub const EIDRM: c_int = 43; /* Identifier removed */
pub const ECHRNG: c_int = 44; /* Channel number out of range */
pub const EL2NSYNC: c_int = 45; /* Level 2 not synchronized */
pub const EL3HLT: c_int = 46; /* Level 3 halted */
pub const EL3RST: c_int = 47; /* Level 3 reset */
pub const ELNRNG: c_int = 48; /* Link number out of range */
pub const EUNATCH: c_int = 49; /* Protocol driver not attached */
pub const ENOCSI: c_int = 50; /* No CSI structure available */
pub const EL2HLT: c_int = 51; /* Level 2 halted */
pub const EBADE: c_int = 52; /* Invalid exchange */
pub const EBADR: c_int = 53; /* Invalid request descriptor */
pub const EXFULL: c_int = 54; /* Exchange full */
pub const ENOANO: c_int = 55; /* No anode */
pub const EBADRQC: c_int = 56; /* Invalid request code */
pub const EBADSLT: c_int = 57; /* Invalid slot */
pub const EDEADLOCK: c_int = 58; /* Resource deadlock would occur */
pub const EBFONT: c_int = 59; /* Bad font file format */
pub const ENOSTR: c_int = 60; /* Device not a stream */
pub const ENODATA: c_int = 61; /* No data available */
pub const ETIME: c_int = 62; /* Timer expired */
pub const ENOSR: c_int = 63; /* Out of streams resources */
pub const ENONET: c_int = 64; /* Machine is not on the network */
pub const ENOPKG: c_int = 65; /* Package not installed */
pub const EREMOTE: c_int = 66; /* Object is remote */
pub const ENOLINK: c_int = 67; /* Link has been severed */
pub const EADV: c_int = 68; /* Advertise error */
pub const ESRMNT: c_int = 69; /* Srmount error */
pub const ECOMM: c_int = 70; /* Communication error on send */
pub const EPROTO: c_int = 71; /* Protocol error */
pub const EMULTIHOP: c_int = 72; /* Multihop attempted */
pub const EDOTDOT: c_int = 73; /* RFS specific error */
pub const EBADMSG: c_int = 74; /* Not a data message */
pub const EOVERFLOW: c_int = 75; /* Value too large for defined data type */
pub const ENOTUNIQ: c_int = 76; /* Name not unique on network */
pub const EBADFD: c_int = 77; /* File descriptor in bad state */
pub const EREMCHG: c_int = 78; /* Remote address changed */
pub const ELIBACC: c_int = 79; /* Can not access a needed shared library */
pub const ELIBBAD: c_int = 80; /* Accessing a corrupted shared library */
pub const ELIBSCN: c_int = 81; /* .lib section in a.out corrupted */
pub const ELIBMAX: c_int = 82; /* Attempting to link in too many shared libraries */
pub const ELIBEXEC: c_int = 83; /* Cannot exec a shared library directly */
pub const EILSEQ: c_int = 84; /* Illegal byte sequence */
pub const ERESTART: c_int = 85; /* Interrupted system call should be restarted */
pub const ESTRPIPE: c_int = 86; /* Streams pipe error */
pub const EUSERS: c_int = 87; /* Too many users */
pub const ENOTSOCK: c_int = 88; /* Socket operation on non-socket */
pub const EDESTADDRREQ: c_int = 89; /* Destination address required */
pub const EMSGSIZE: c_int = 90; /* Message too long */
pub const EPROTOTYPE: c_int = 91; /* Protocol wrong type for socket */
pub const ENOPROTOOPT: c_int = 92; /* Protocol not available */
pub const EPROTONOSUPPORT: c_int = 93; /* Protocol not supported */
pub const ESOCKTNOSUPPORT: c_int = 94; /* Socket type not supported */
pub const EOPNOTSUPP: c_int = 95; /* Operation not supported on transport endpoint */
pub const EPFNOSUPPORT: c_int = 96; /* Protocol family not supported */
pub const EAFNOSUPPORT: c_int = 97; /* Address family not supported by protocol */
pub const EADDRINUSE: c_int = 98; /* Address already in use */
pub const EADDRNOTAVAIL: c_int = 99; /* Cannot assign requested address */
pub const ENETDOWN: c_int = 100; /* Network is down */
pub const ENETUNREACH: c_int = 101; /* Network is unreachable */
pub const ENETRESET: c_int = 102; /* Network dropped connection because of reset */
pub const ECONNABORTED: c_int = 103; /* Software caused connection abort */
pub const ECONNRESET: c_int = 104; /* Connection reset by peer */
pub const ENOBUFS: c_int = 105; /* No buffer space available */
pub const EISCONN: c_int = 106; /* Transport endpoint is already connected */
pub const ENOTCONN: c_int = 107; /* Transport endpoint is not connected */
pub const ESHUTDOWN: c_int = 108; /* Cannot send after transport endpoint shutdown */
pub const ETOOMANYREFS: c_int = 109; /* Too many references: cannot splice */
pub const ETIMEDOUT: c_int = 110; /* Connection timed out */
pub const ECONNREFUSED: c_int = 111; /* Connection refused */
pub const EHOSTDOWN: c_int = 112; /* Host is down */
pub const EHOSTUNREACH: c_int = 113; /* No route to host */
pub const EALREADY: c_int = 114; /* Operation already in progress */
pub const EINPROGRESS: c_int = 115; /* Operation now in progress */
pub const ESTALE: c_int = 116; /* Stale NFS file handle */
pub const EUCLEAN: c_int = 117; /* Structure needs cleaning */
pub const ENOTNAM: c_int = 118; /* Not a XENIX named type file */
pub const ENAVAIL: c_int = 119; /* No XENIX semaphores available */
pub const EISNAM: c_int = 120; /* Is a named type file */
pub const EREMOTEIO: c_int = 121; /* Remote I/O error */
pub const EDQUOT: c_int = 122; /* Quota exceeded */
pub const ENOMEDIUM: c_int = 123; /* No medium found */
pub const EMEDIUMTYPE: c_int = 124; /* Wrong medium type */
pub const ECANCELED: c_int = 125; /* Operation Canceled */
pub const ENOKEY: c_int = 126; /* Required key not available */
pub const EKEYEXPIRED: c_int = 127; /* Key has expired */
pub const EKEYREVOKED: c_int = 128; /* Key has been revoked */
pub const EKEYREJECTED: c_int = 129; /* Key was rejected by service */
pub const EOWNERDEAD: c_int = 130; /* Owner died */
pub const ENOTRECOVERABLE: c_int = 131; /* State not recoverable */

pub static STR_ERROR: [&'static str; 132] = [
    "Success",
    "Operation not permitted",
    "No such file or directory",
    "No such process",
    "Interrupted system call",
    "I/O error",
    "No such device or address",
    "Argument list too long",
    "Exec format error",
    "Bad file number",
    "No child processes",
    "Try again",
    "Out of memory",
    "Permission denied",
    "Bad address",
    "Block device required",
    "Device or resource busy",
    "File exists",
    "Cross-device link",
    "No such device",
    "Not a directory",
    "Is a directory",
    "Invalid argument",
    "File table overflow",
    "Too many open files",
    "Not a typewriter",
    "Text file busy",
    "File too large",
    "No space left on device",
    "Illegal seek",
    "Read-only file system",
    "Too many links",
    "Broken pipe",
    "Math argument out of domain of func",
    "Math result not representable",
    "Resource deadlock would occur",
    "File name too long",
    "No record locks available",
    "Function not implemented",
    "Directory not empty",
    "Too many symbolic links encountered",
    "Operation would block",
    "No message of desired type",
    "Identifier removed",
    "Channel number out of range",
    "Level 2 not synchronized",
    "Level 3 halted",
    "Level 3 reset",
    "Link number out of range",
    "Protocol driver not attached",
    "No CSI structure available",
    "Level 2 halted",
    "Invalid exchange",
    "Invalid request descriptor",
    "Exchange full",
    "No anode",
    "Invalid request code",
    "Invalid slot",
    "Resource deadlock would occur",
    "Bad font file format",
    "Device not a stream",
    "No data available",
    "Timer expired",
    "Out of streams resources",
    "Machine is not on the network",
    "Package not installed",
    "Object is remote",
    "Link has been severed",
    "Advertise error",
    "Srmount error",
    "Communication error on send",
    "Protocol error",
    "Multihop attempted",
    "RFS specific error",
    "Not a data message",
    "Value too large for defined data type",
    "Name not unique on network",
    "File descriptor in bad state",
    "Remote address changed",
    "Can not access a needed shared library",
    "Accessing a corrupted shared library",
    ".lib section in a.out corrupted",
    "Attempting to link in too many shared libraries",
    "Cannot exec a shared library directly",
    "Illegal byte sequence",
    "Interrupted system call should be restarted",
    "Streams pipe error",
    "Too many users",
    "Socket operation on non-socket",
    "Destination address required",
    "Message too long",
    "Protocol wrong type for socket",
    "Protocol not available",
    "Protocol not supported",
    "Socket type not supported",
    "Operation not supported on transport endpoint",
    "Protocol family not supported",
    "Address family not supported by protocol",
    "Address already in use",
    "Cannot assign requested address",
    "Network is down",
    "Network is unreachable",
    "Network dropped connection because of reset",
    "Software caused connection abort",
    "Connection reset by peer",
    "No buffer space available",
    "Transport endpoint is already connected",
    "Transport endpoint is not connected",
    "Cannot send after transport endpoint shutdown",
    "Too many references: cannot splice",
    "Connection timed out",
    "Connection refused",
    "Host is down",
    "No route to host",
    "Operation already in progress",
    "Operation now in progress",
    "Stale NFS file handle",
    "Structure needs cleaning",
    "Not a XENIX named type file",
    "No XENIX semaphores available",
    "Is a named type file",
    "Remote I/O error",
    "Quota exceeded",
    "No medium found",
    "Wrong medium type",
    "Operation Canceled",
    "Required key not available",
    "Key has expired",
    "Key has been revoked",
    "Key was rejected by service",
    "Owner died",
    "State not recoverable",
];
