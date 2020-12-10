//! This crate creates the abstraction of `stdio`. They are essentially ring buffer of bytes.
//! It also creates the queue for `KeyEvent`, which allows applications to have direct access
//! to keyboard events.
#![no_std]

extern crate alloc;
extern crate spin;
extern crate bare_io;
extern crate keycodes_ascii;

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
use bare_io::{Read, Write};
use keycodes_ascii::KeyEvent;
use core::ops::Deref;

/// A ring buffer with an EOF mark.
pub struct RingBufferEof<T> {
    /// The ring buffer.
    queue: VecDeque<T>,
    /// The EOF mark. We meet EOF when it equals `true`.
    end: bool
}

/// A reference to a ring buffer with an EOF mark with mutex protection.
pub type RingBufferEofRef<T> = Arc<Mutex<RingBufferEof<T>>>;

/// A ring buffer containing bytes. It forms `stdin`, `stdout` and `stderr`.
/// The two `Arc`s actually point to the same ring buffer. It is designed to prevent
/// interleaved reading but at the same time allow writing to the ring buffer while
/// the reader is holding its lock, and vice versa.
pub struct Stdio {
    /// This prevents interleaved reading.
    read_access: Arc<Mutex<RingBufferEofRef<u8>>>,
    /// This prevents interleaved writing.
    write_access: Arc<Mutex<RingBufferEofRef<u8>>>
}

/// A reader to stdio buffers.
#[derive(Clone)]
pub struct StdioReader {
    /// Inner buffer to support buffered read.
    inner_buf: Box<[u8]>,
    /// The length of actual buffered bytes.
    inner_content_len: usize,
    /// Points to the ring buffer.
    read_access: Arc<Mutex<RingBufferEofRef<u8>>>
}

/// A writer to stdio buffers.
#[derive(Clone)]
pub struct StdioWriter {
    /// Points to the ring buffer.
    write_access: Arc<Mutex<RingBufferEofRef<u8>>>
}

/// `StdioReadGuard` acts like `MutexGuard`, it locks the underlying ring buffer during its
/// lifetime, and provides reading methods to the ring buffer. The lock will be automatically
/// released on dropping of this structure.
pub struct StdioReadGuard<'a> {
    guard: MutexGuard<'a, RingBufferEofRef<u8>>
}

/// `StdioReadGuard` acts like `MutexGuard`, it locks the underlying ring buffer during its
/// lifetime, and provides writing methods to the ring buffer. The lock will be automatically
/// released on dropping of this structure.
pub struct StdioWriteGuard<'a> {
    guard: MutexGuard<'a, RingBufferEofRef<u8>>
}

impl<T> RingBufferEof<T> {
    /// Create a new ring buffer.
    fn new() -> RingBufferEof<T> {
        RingBufferEof {
            queue: VecDeque::new(),
            end: false
        }
    }
}

impl Stdio {
    /// Create a new stdio buffer.
    pub fn new() -> Stdio {
        let ring_buffer = Arc::new(Mutex::new(RingBufferEof::new()));
        Stdio {
            read_access: Arc::new(Mutex::new(Arc::clone(&ring_buffer))),
            write_access: Arc::new(Mutex::new(ring_buffer))
        }
    }

    /// Get a reader to the stdio buffer. Note that each reader has its own
    /// inner buffer. The buffer size is set to be 256 bytes. Resort to
    /// `get_reader_with_buf_capacity` if one needs a different buffer size.
    pub fn get_reader(&self) -> StdioReader {
        StdioReader {
            inner_buf: Box::new([0u8; 256]),
            inner_content_len: 0,
            read_access: Arc::clone(&self.read_access)
        }
    }

    /// Get a reader to the stdio buffer with a customized buffer size.
    /// Note that each reader has its own inner buffer.
    pub fn get_reader_with_buf_capacity(&self, capacity: usize) -> StdioReader {
        let mut inner_buf = Vec::with_capacity(capacity);
        inner_buf.resize(capacity, 0u8);
        StdioReader {
            inner_buf: inner_buf.into_boxed_slice(),
            inner_content_len: 0,
            read_access: Arc::clone(&self.read_access)
        }
    }

    /// Get a writer to the stdio buffer.
    pub fn get_writer(&self) -> StdioWriter {
        StdioWriter {
            write_access: Arc::clone(&self.write_access)
        }
    }
}

impl StdioReader {
    /// Lock the reader and return a guard that can perform reading operation to that buffer.
    /// Note that this lock does not lock the underlying ring buffer. It only excludes other
    /// readr from performing simultaneous read, but does *not* prevent a writer to perform
    /// writing to the underlying ring buffer.
    pub fn lock(&self) -> StdioReadGuard {
        StdioReadGuard {
            guard: self.read_access.lock()
        }
    }

    /// Read a line from the ring buffer and return. Remaining bytes are stored in the inner
    /// buffer. Do NOT use this function alternatively with `read()` method defined in
    /// `StdioReadGuard`. This function returns the number of bytes read. It will return
    /// zero only upon EOF.
    pub fn read_line(&mut self, buf: &mut String) -> Result<usize, bare_io::Error> {
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
                self.inner_content_len = new_cnt - cnt_before_new_line;
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

impl StdioWriter {
    /// Lock the writer and return a guard that can perform writing operation to that buffer.
    /// Note that this lock does not lock the underlying ring buffer. It only excludes other
    /// writer from performing simultaneous write, but does *not* prevent a reader to perform
    /// reading to the underlying ring buffer.
    pub fn lock(&self) -> StdioWriteGuard {
        StdioWriteGuard {
            guard: self.write_access.lock()
        }
    }
}

impl<'a> Read for StdioReadGuard<'a> {
    /// Read from the ring buffer. Returns the number of bytes read. 
    /// 
    /// Currently it is not possible to return an error, 
    /// but one should *not* assume that because it is subject to change in the future.
    /// 
    /// Note that this method will block until at least one byte is available to be read.
    /// It will only return zero under one of two scenarios:
    /// 1. The EOF flag has been set.
    /// 2. The buffer specified was 0 bytes in length.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, bare_io::Error> {

        // Deal with the edge case that the buffer specified was 0 bytes in length.
        if buf.len() == 0 { return Ok(0); }

        let mut cnt: usize = 0;
        loop {
            let end; // EOF flag
            {
                let mut locked_ring_buf = self.guard.lock();
                let mut buf_iter = buf[cnt..].iter_mut();

                // Keep reading if we have empty space in the output buffer
                // and available byte in the ring buffer.
                while let Some(buf_entry) = buf_iter.next() {
                    if let Some(queue_elem) = locked_ring_buf.queue.pop_front() {
                        *buf_entry = queue_elem;
                        cnt += 1;
                    } else {
                        break;
                    }
                }

                end = locked_ring_buf.end;
            } // the lock on the ring buffer is guaranteed to be dropped here

            // Break if we have read something or we encounter EOF.
            if cnt > 0 || end { break; }
        }
        return Ok(cnt);
    }
}

impl<'a> StdioReadGuard<'a> {
    /// Same as `read()`, but is non-blocking.
    /// 
    /// Returns `Ok(0)` when the underlying buffer is empty.
    pub fn try_read(&mut self, buf: &mut [u8]) -> Result<usize, bare_io::Error> {

        // Deal with the edge case that the buffer specified was 0 bytes in length.
        if buf.len() == 0 { return Ok(0); }

        let mut buf_iter = buf.iter_mut();
        let mut cnt: usize = 0;
        let mut locked_ring_buf = self.guard.lock();

        // Keep reading if we have empty space in the output buffer
        // and available byte in the ring buffer.
        while let (Some(buf_elem), Some(queue_elem)) = (buf_iter.next(), locked_ring_buf.queue.pop_front()) {
            *buf_elem = queue_elem;
            cnt += 1;
        }

        return Ok(cnt);
    }

    /// Returns the number of bytes still in the read buffer.
    pub fn remaining_bytes(&self) -> usize {
        return self.guard.lock().queue.len();
    }
}

impl<'a> Write for StdioWriteGuard<'a> {
    /// Write to the ring buffer, returniong the number of bytes written.
    /// 
    /// When this method is called after setting the EOF flag, it returns error with `ErrorKind`
    /// set to `UnexpectedEof`.
    /// 
    /// Also note that this method does *not* guarantee to write all given bytes, although it currently
    /// does so. Always check the return value when using this method. Otherwise, use `write_all` to
    /// ensure that all given bytes are written.
    fn write(&mut self, buf: &[u8]) -> Result<usize, bare_io::Error> {
        if self.guard.lock().end {
            return Err(bare_io::Error::new(bare_io::ErrorKind::UnexpectedEof,
                                           "cannot write to a stream with EOF set"));
        }
        let mut locked_ring_buf = self.guard.lock();
        for byte in buf {
            locked_ring_buf.queue.push_back(*byte)
        }
        Ok(buf.len())
    }
    /// The function required by `Write` trait. Currently it performs nothing,
    /// since everything is write directly to the ring buffer in `write` method.
    fn flush(&mut self) -> Result<(), bare_io::Error> {
        Ok(())
    }
}

impl<'a> StdioReadGuard<'a> {
    /// Check if the EOF flag of the queue has been set.
    pub fn is_eof(&self) -> bool {
        self.guard.lock().end
    }
}

impl<'a> StdioWriteGuard<'a> {
    /// Set the EOF flag of the queue to true.
    pub fn set_eof(&mut self) {
        self.guard.lock().end = true;
    }
}

pub struct KeyEventQueue {
    /// A ring buffer storing `KeyEvent`.
    key_event_queue: RingBufferEofRef<KeyEvent>
}

/// A reader to keyevent ring buffer.
#[derive(Clone)]
pub struct KeyEventQueueReader {
    /// Points to the ring buffer storing `KeyEvent`.
    key_event_queue: RingBufferEofRef<KeyEvent>
}

/// A writer to keyevent ring buffer.
#[derive(Clone)]
pub struct KeyEventQueueWriter {
    /// Points to the ring buffer storing `KeyEvent`.
    key_event_queue: RingBufferEofRef<KeyEvent>
}

impl KeyEventQueue {
    /// Create a new ring buffer storing `KeyEvent`.
    pub fn new() -> KeyEventQueue {
        KeyEventQueue {
            key_event_queue: Arc::new(Mutex::new(RingBufferEof::new()))
        }
    }

    /// Get a reader to the ring buffer.
    pub fn get_reader(&self) -> KeyEventQueueReader {
        KeyEventQueueReader {
            key_event_queue: self.key_event_queue.clone()
        }
    }

    /// Get a writer to the ring buffer.
    pub fn get_writer(&self) -> KeyEventQueueWriter {
        KeyEventQueueWriter {
            key_event_queue: self.key_event_queue.clone()
        }
    }
}

impl KeyEventQueueReader {
    /// Try to read a keyevent from the ring buffer. It returns `None` if currently
    /// the ring buffer is empty.
    pub fn read_one(&self) -> Option<KeyEvent> {
        let mut locked_queue = self.key_event_queue.lock();
        locked_queue.queue.pop_front()
    }
}

impl KeyEventQueueWriter {
    /// Push a keyevent into the ring buffer.
    pub fn write_one(&self, key_event: KeyEvent) {
        let mut locked_queue = self.key_event_queue.lock();
        locked_queue.queue.push_back(key_event);
    }
}

/// A structure that allows applications to access keyboard events directly. 
/// When it gets instantiated, it `take`s the reader of the `KeyEventQueue` away from the `shell`, 
/// or whichever entity previously owned the queue.
/// When it goes out of the scope, the taken reader will be automatically returned
/// back to the `shell` or the original owner in its `Drop` routine.
pub struct KeyEventReadGuard {
    /// The taken reader of the `KeyEventQueue`.
    reader: Option<KeyEventQueueReader>,
    /// The closure to be excuted on dropping.
    closure: Box<dyn Fn(&mut Option<KeyEventQueueReader>)>
}

impl KeyEventReadGuard {
    /// Create a new `KeyEventReadGuard`. This function *takes* a reader
    /// to `KeyEventQueue`. Thus, the `reader` will never be `None` until the
    /// `drop()` method.
    pub fn new(
        reader: KeyEventQueueReader,
        closure: Box<dyn Fn(&mut Option<KeyEventQueueReader>)>
    ) -> KeyEventReadGuard {
        KeyEventReadGuard {
            reader: Some(reader),
            closure
        }
    }
}

impl Drop for KeyEventReadGuard {
    /// Returns the reader of `KeyEventQueue` back to the previous owner by executing the closure.
    fn drop(&mut self) {
        (self.closure)(&mut self.reader);
    }
}

impl Deref for KeyEventReadGuard {
    type Target = Option<KeyEventQueueReader>;

    fn deref(&self) -> &Self::Target {
        &self.reader
    }
}
