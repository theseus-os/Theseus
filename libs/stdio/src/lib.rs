//! This crate creates the abstraction of `stdio`. They are essentially ring buffer of bytes.
//! It also creates the queue for `KeyEvent`, which allows applications to have direct access
//! to keyboard events.
#![no_std]

extern crate alloc;
extern crate spin;
extern crate core_io;
extern crate keycodes_ascii;

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
use core_io::{Read, Write};
use keycodes_ascii::KeyEvent;
use core::ops::Deref;

/// A ring buffer with an EOF mark.
struct IoCoreBuffer<T> {
    /// The ring buffer.
    queue: VecDeque<T>,
    /// The EOF mark. We meet EOF when it equals `true`.
    end: bool
}

/// A ring buffer protected by mutex.
struct IoCore<T> {
    mtx: Mutex<IoCoreBuffer<T>>
}

/// A ring buffer containing bytes. It forms `stdin`, `stdout` and `stderr`.
pub struct Stdio {
    core: Arc<IoCore<u8>>,
    read_handle_lock: Arc<Mutex<()>>,
    write_handle_lock: Arc<Mutex<()>>
}

/// A read handle to stdio buffers.
#[derive(Clone)]
pub struct StdioReadHandle {
    /// Inner buffer to support buffered read.
    inner_buf: Vec<u8>,
    /// The length of actual buffered bytes.
    inner_content_len: usize,
    /// Points to the ring buffer.
    core: Arc<IoCore<u8>>,
    read_handle_lock: Arc<Mutex<()>>
}

/// A write handle to stdio buffers.
#[derive(Clone)]
pub struct StdioWriteHandle {
    /// Points to the ring buffer.
    core: Arc<IoCore<u8>>,
    write_handle_lock: Arc<Mutex<()>>
}

/// `StdioReadGuard` acts like `MutexGuard`, it locks the underlying ring buffer during its
/// lifetime, and provides reading methods to the ring buffer. The lock will be automatically
/// released on dropping of this structure.
pub struct StdioReadGuard<'a> {
    _guard: MutexGuard<'a, ()>,
    core: Arc<IoCore<u8>>
}

/// `StdioReadGuard` acts like `MutexGuard`, it locks the underlying ring buffer during its
/// lifetime, and provides writing methods to the ring buffer. The lock will be automatically
/// released on dropping of this structure.
pub struct StdioWriteGuard<'a> {
    _guard: MutexGuard<'a, ()>,
    core: Arc<IoCore<u8>>
}

impl<T> IoCoreBuffer<T> {
    /// Create a new ring buffer.
    fn new() -> IoCoreBuffer<T> {
        IoCoreBuffer {
            queue: VecDeque::new(),
            end: false
        }
    }
}

impl<T> IoCore<T> {
    /// Create a new ring buffer enclosed by a mutex.
    fn new() -> IoCore<T> {
        IoCore {
            mtx: Mutex::new(IoCoreBuffer::new())
        }
    }
}

impl Stdio {
    /// Create a new stdio buffer.
    pub fn new() -> Stdio {
        Stdio {
            core: Arc::new(IoCore::new()),
            read_handle_lock: Arc::new(Mutex::new(())),
            write_handle_lock: Arc::new(Mutex::new(()))
        }
    }

    /// Get a read handle to the stdio buffer. Note that each read handle has its own
    /// inner buffer. The buffer size is set to be 256 bytes. Resort to
    /// `get_read_handle_with_buf_capacity` if one needs a different buffer size.
    pub fn get_read_handle(&self) -> StdioReadHandle {
        let mut inner_buf = Vec::new();
        inner_buf.resize(256, 0);
        StdioReadHandle {
            inner_buf,
            inner_content_len: 0,
            core: self.core.clone(),
            read_handle_lock: Arc::clone(&self.read_handle_lock)
        }
    }

    /// Get a read handle to the stdio buffer with a customized buffer size.
    /// Note that each read handle has its own inner buffer.
    pub fn get_read_handle_with_buf_capacity(&self, capacity: usize) -> StdioReadHandle {
        let mut inner_buf = Vec::new();
        inner_buf.resize(capacity, 0);
        StdioReadHandle {
            inner_buf,
            inner_content_len: 0,
            core: self.core.clone(),
            read_handle_lock: Arc::clone(&self.read_handle_lock)
        }
    }

    /// Get a write handle to the stdio buffer.
    pub fn get_write_handle(&self) -> StdioWriteHandle {
        StdioWriteHandle {
            core: self.core.clone(),
            write_handle_lock: Arc::clone(&self.write_handle_lock)
        }
    }
}

impl StdioReadHandle {
    /// Lock the read handle and return a guard that can perform reading operation to that buffer.
    /// Note that this lock does not lock the underlying ring buffer. It only excludes other
    /// read handle from performing simultaneous read, but does *not* prevent a write handle
    /// to perform writing to the underlying ring buffer.
    pub fn lock(&self) -> StdioReadGuard {
        StdioReadGuard {
            _guard: self.read_handle_lock.lock(),
            core: Arc::clone(&self.core)
        }
    }

    /// Read a line from the ring buffer and return. Remaining bytes are stored in the inner
    /// buffer. Do NOT use this function alternatively with `read()` method defined in
    /// `StdioReadGuard`. This function returns the number of bytes read.
    pub fn read_line(&mut self, buf: &mut String) -> Result<usize, core_io::Error> {
        let mut total_cnt = 0usize;    // total number of bytes read this time
        let mut new_cnt;               // number of bytes returned from a `read()` invocation
        let mut tmp_buf = Vec::new();  // temporary buffer
        let mut line_finished = false; // mark if we have finished a line

        // Copy from the inner buffer. Process the remaining characters from last read first.
        tmp_buf.resize(self.inner_buf.len(), 0);
        tmp_buf[0..self.inner_content_len].clone_from_slice(&self.inner_buf[0..self.inner_content_len]);
        new_cnt = self.inner_content_len;
        self.inner_content_len = 0;

        loop {
            // Try to find an '\n' character.
            let mut cnt_before_new_line = new_cnt;
            for (idx, c) in tmp_buf[0..new_cnt].iter().enumerate() {
                if *c as char == '\n' {
                    cnt_before_new_line = idx + 1;
                    line_finished = true;
                    break;
                }
            }

            // Append new characters to output buffer (until '\n').
            total_cnt += cnt_before_new_line;
            let new_str = String::from_utf8_lossy(&tmp_buf[0..cnt_before_new_line]);
            buf.push_str(&new_str);

            // If we have read a whole line, copy any byte left to inner buffer, and then return.
            if line_finished {
                self.inner_buf[0..new_cnt-cnt_before_new_line].clone_from_slice(&tmp_buf[cnt_before_new_line..new_cnt]);
                return Ok(total_cnt);
            }

            // We have not finished a whole line. Try to read more from the ring buffer, until
            // we hit EOF.
            let mut locked = self.lock();
            new_cnt = locked.read(&mut tmp_buf[..])?;
            if new_cnt == 0 && locked.is_eof() { return Ok(total_cnt); }
        }
    }
}

impl StdioWriteHandle {
    /// Lock the write handle and return a guard that can perform writing operation to that buffer.
    /// Note that this lock does not lock the underlying ring buffer. It only excludes other
    /// write handle from performing simultaneous write, but does *not* prevent a read handle
    /// to perform reading to the underlying ring buffer.
    pub fn lock(&self) -> StdioWriteGuard {
        StdioWriteGuard {
            _guard: self.write_handle_lock.lock(),
            core: Arc::clone(&self.core)
        }
    }
}

impl<'a> Read for StdioReadGuard<'a> {
    /// Read from the ring buffer. It returns the number of bytes read. Currently it is not possible
    /// to return an error, but one should *not* simply unwrap the return value since the implementation
    /// detail is subjected to change in the future.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, core_io::Error> {
        let mut buf_iter = buf.iter_mut();
        let mut cnt: usize = 0;
        let mut locked_core = self.core.mtx.lock();

        // Keep reading if we have empty space in the output buffer
        // and available byte in the ring buffer.
        while let (Some(buf_elem), Some(queue_elem)) = (buf_iter.next(), locked_core.queue.pop_front()) {
            *buf_elem = queue_elem;
            cnt += 1;
        }
        return Ok(cnt);
    }
}

impl<'a> Write for StdioWriteGuard<'a> {
    /// Write to the ring buffer. It returns the number of bytes written. Currently it is not possible
    /// to return an error, but one should *not* simply unwrap the return value since the implementation
    /// detail is subjected to change in the future.
    /// 
    /// Also note that this method does *not* guarantee to write all given bytes, although it currently
    /// does so. Always check the return value when using this method. Otherwise, use `write_all` to
    /// ensure that all given bytes are written.
    fn write(&mut self, buf: &[u8]) -> Result<usize, core_io::Error> {
        let mut locked_core = self.core.mtx.lock();
        for byte in buf {
            locked_core.queue.push_back(*byte)
        }
        Ok(buf.len())
    }
    /// The function required by `Write` trait. Currently it performs nothing,
    /// since everything is write directly to the ring buffer in `write` method.
    fn flush(&mut self) -> Result<(), core_io::Error> {
        Ok(())
    }
}

impl<'a> StdioReadGuard<'a> {
    /// Check if the EOF flag of the queue has been set.
    pub fn is_eof(&self) -> bool {
        self.core.mtx.lock().end
    }
}

impl<'a> StdioWriteGuard<'a> {
    /// Set the EOF flag of the queue to true.
    pub fn set_eof(&mut self) {
        self.core.mtx.lock().end = true;
    }
}

pub struct KeyEventQueue {
    /// A ring buffer storing `KeyEvent`.
    key_event_queue: Arc<IoCore<KeyEvent>>
}

/// A read handle to keyevent ring buffer.
#[derive(Clone)]
pub struct KeyEventQueueReadHandle {
    /// Points to the ring buffer storing `KeyEvent`.
    key_event_queue: Arc<IoCore<KeyEvent>>
}

/// A write handle to keyevent ring buffer.
#[derive(Clone)]
pub struct KeyEventQueueWriteHandle {
    /// Points to the ring buffer storing `KeyEvent`.
    key_event_queue: Arc<IoCore<KeyEvent>>
}

impl KeyEventQueue {
    /// Create a new ring buffer storing `KeyEvent`.
    pub fn new() -> KeyEventQueue {
        KeyEventQueue {
            key_event_queue: Arc::new(IoCore::new())
        }
    }

    /// Get a read handle to the ring buffer.
    pub fn get_read_handle(&self) -> KeyEventQueueReadHandle {
        KeyEventQueueReadHandle {
            key_event_queue: self.key_event_queue.clone()
        }
    }

    /// Get a write handle to the ring buffer.
    pub fn get_write_handle(&self) -> KeyEventQueueWriteHandle {
        KeyEventQueueWriteHandle {
            key_event_queue: self.key_event_queue.clone()
        }
    }
}

impl KeyEventQueueReadHandle {
    /// Try to read a keyevent from the ring buffer. It returns `None` if currently
    /// the ring buffer is empty.
    pub fn read_one(&self) -> Option<KeyEvent> {
        let mut locked_queue = self.key_event_queue.mtx.lock();
        locked_queue.queue.pop_front()
    }
}

impl KeyEventQueueWriteHandle {
    /// Push a keyevent into the ring buffer.
    pub fn write_one(&self, key_event: KeyEvent) {
        let mut locked_queue = self.key_event_queue.mtx.lock();
        locked_queue.queue.push_back(key_event);
    }
}

/// A structure that allows applications to access keyboard events directly. When
/// it get instantiated, it *takes* the read handle of the `KeyEventQueue`. When it
/// goes out of the scope, the taken read handle will be automatically returned back
/// to the shell by `drop()` method.
pub struct KeyEventConsumerGuard {
    /// The taken read handle of the `KeyEventQueue`.
    read_handle: Option<KeyEventQueueReadHandle>,
    /// The closure to be excuted on dropping.
    closure: Box<Fn(KeyEventQueueReadHandle)>
}

impl KeyEventConsumerGuard {
    /// Create a new `KeyEventConsumerGuard`. This function *takes* a read handle
    /// to `KeyEventQueue`. Thus, the `read_handle` will never be `None` until the
    /// `drop()` method. We can safely `unwrap()` the `read_handle` field.
    pub fn new(read_handle: KeyEventQueueReadHandle,
               closure: Box<Fn(KeyEventQueueReadHandle)>) -> KeyEventConsumerGuard {
        KeyEventConsumerGuard {
            read_handle: Some(read_handle),
            closure
        }
    }
}

impl Drop for KeyEventConsumerGuard {
    /// Returns the read handle of `KeyEventQueue` back to shell by executing the
    /// closure. Note that `read_handle` will never be `None` before `drop()`. So we
    /// can safely call `unwrap()` here. See `new()` method for details.
    fn drop(&mut self) {
        (self.closure)(self.read_handle.take().unwrap());
    }
}

impl Deref for KeyEventConsumerGuard {
    type Target = KeyEventQueueReadHandle;

    /// It allows us to access the read handle with dot operator. Note that `read_handle`
    /// will never be `None` before `drop()`. So we can safely call `unwrap()` here. See
    /// `new()` method for details.
    fn deref(&self) -> &Self::Target {
        self.read_handle.as_ref().unwrap()
    }
}
