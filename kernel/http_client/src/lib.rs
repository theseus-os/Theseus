//! Functions for creating and sending HTTP requests and receiving responses.
//! 

#![no_std]
#![feature(alloc)]
#![feature(try_from)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate smoltcp;
extern crate network_manager;
extern crate spin;
extern crate acpi;
extern crate httparse;
extern crate percent_encoding;


use core::convert::TryInto;
use core::str;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Once;
use acpi::get_hpet;
use smoltcp::{
    socket::{SocketSet, TcpSocket, SocketHandle},
    time::Instant
};
use percent_encoding::{DEFAULT_ENCODE_SET, utf8_percent_encode};
use network_manager::{NetworkInterfaceRef};


/// The IP address of the update server.
/// This is currently the static IP of `kevin.recg.rice.edu`.
const DEFAULT_DESTINATION_IP_ADDR: [u8; 4] = [168, 7, 138, 84];

/// The TCP port on the update server that listens for update requests 
const DEFAULT_DESTINATION_PORT: u16 = 8090;

/// The starting number for freely-available (non-reserved) standard TCP/UDP ports.
const STARTING_FREE_PORT: u16 = 49152;



/// The states that implement the finite state machine for 
/// sending and receiving the HTTP request and response, respectively.
#[derive(Debug)]
enum HttpState {
    /// The socket is connected, but the HTTP request has not yet been sent.
    Requesting,
    /// The HTTP request has been sent, but the response has not yet been fully received.
    ReceivingResponse,
    /// The response has been received in full, including the headers and the entire content.
    Responded
}


/// A simple macro to get the current HPET clock ticks.
#[macro_export]
macro_rules! hpet_ticks {
    () => {
        get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter()
    };
}


/// Function to calculate the currently elapsed time (in milliseconds) since the given `start_time` (also milliseconds).
pub fn millis_since(start_time: u64) -> Result<u64, &'static str> {
    const FEMTOSECONDS_PER_MILLISECOND: u64 = 1_000_000_000_000;
    static HPET_PERIOD_FEMTOSECONDS: Once<u32> = Once::new();

    let hpet_freq = match HPET_PERIOD_FEMTOSECONDS.try() {
        Some(period) => period,
        _ => {
            let freq = get_hpet().as_ref().ok_or("couldn't get HPET")?.counter_period_femtoseconds();
            HPET_PERIOD_FEMTOSECONDS.call_once(|| freq)
        }
    };
    let hpet_freq = *hpet_freq as u64;

    let end_time: u64 = hpet_ticks!();
    // Convert to ms
    let diff = (end_time - start_time) * hpet_freq / FEMTOSECONDS_PER_MILLISECOND;
    Ok(diff)
}


/// A convenience function to poll the given network interface (i.e., flush tx/rx).
/// Returns true if any packets were sent or received through that interface on the given `sockets`.
pub fn poll_iface(iface: &NetworkInterfaceRef, sockets: &mut SocketSet, startup_time: u64) -> Result<bool, &'static str> {
    let timestamp: i64 = millis_since(startup_time)?
        .try_into()
        .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
    let packets_were_sent_or_received = match iface.lock().poll(sockets, Instant::from_millis(timestamp)) {
        Ok(b) => b,
        Err(err) => {
            warn!("http_client: poll error: {}", err);
            false
        }
    };
    Ok(packets_were_sent_or_received)
}


/// Checks to see if the provided HTTP request can be properly parsed, and returns true if so.
pub fn check_http_request(request_bytes: &[u8]) -> bool {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    request.parse(request_bytes).is_ok() && request_bytes.ends_with(b"\r\n\r\n")
}


/// TODO FIXME: create a proper HttpRequest type with header creation fields and actual verification
pub type HttpRequest = String;


/// An HttpResponse that has been fully received from a remote server.
pub struct HttpResponse {
    /// The actual array of raw bytes received from the server, 
    /// including all of the headers and body.
    pub packet: Vec<u8>,
    /// The length of all headers
    pub header_length: usize,
}
impl HttpResponse {
    pub fn header_bytes(&self) -> &[u8] {
        &self.packet[0 .. self.header_length]
    }

    pub fn content(&self) -> &[u8] {
        &self.packet[self.header_length ..]
    }
}


/// A convenience struct that packages together a connected TCP socket
/// with other elements that are necessary to transceive packets. 
pub struct ConnectedTcpSocket<'i, 's, 'sockset_a, 'sockset_b, 'sockset_c> {
    iface:   &'i NetworkInterfaceRef, 
    sockets: &'s mut SocketSet<'sockset_a, 'sockset_b, 'sockset_c>,
    handle:  SocketHandle,
}
impl<'i, 's, 'sockset_a, 'sockset_b, 'sockset_c> ConnectedTcpSocket<'i, 's, 'sockset_a, 'sockset_b, 'sockset_c> {
    /// Create a new `ConnectedTcpSocket` with the given necessary items:
    /// # Arguments
    /// * `iface`: a reference to the `NetworkInterface` that the given TCP socket was created on and uses for transceiving packets. 
    /// * `sockets`: the set of sockets that includes the given TCP socket (usually just a set with just that one socket).
    /// * `tcp_socket_handle`: the handle of the TCP socket, which must be in the given `sockets` set and be already connected to the remote endpoint.
    /// 
    /// Returns an `Err` result if the TCP socket isn't connected to the remote endpoint.
    /// 
    pub fn new(
        iface: &'i NetworkInterfaceRef, 
        sockets: &'s mut SocketSet<'sockset_a, 'sockset_b, 'sockset_c>,
        tcp_socket_handle: SocketHandle, 
    ) -> Result<ConnectedTcpSocket<'i, 's, 'sockset_a, 'sockset_b, 'sockset_c>, &'static str> {
        // ensure the socket actually connected to the remote endpoint (i.e., it should be able to send/recv)
        {
            let socket = sockets.get::<TcpSocket>(tcp_socket_handle);
            let connected = socket.may_send() && socket.may_recv();
            if !connected {
                return Err("http_client: the given TCP socket wasn't connected to the remote endpoint");
            }
        }

        Ok(ConnectedTcpSocket {
            iface: iface,
            sockets: sockets,
            handle: tcp_socket_handle,
        })
    }
}

/// Sends the given HTTP request over the network via the given `socket` on the given `interface`,
/// waits to receive a full HTTP response from the remote server, 
/// and then returns that full response, or an error if the response wasn't fully received properly.
/// 
/// # Arguments
/// * `request`: the HTTP request to be sent via the connected socket.
/// * `tcp_socket`: the connected TCP socket that will be used to send the HTTP request and receive the response.
/// * `timeout_millis`: the timeout in milliseconds that limits how long this function will wait for a response from the remote endpoint.
/// 
pub fn send_request(
    request: HttpRequest, 
    tcp_socket: &mut ConnectedTcpSocket,
    timeout_millis: Option<u64>,
) -> Result<HttpResponse, &'static str> {

    // validate the HTTP request 
    if !check_http_request(request.as_bytes()) {
        return Err("http_client: given HTTP request was improperly formatted or incomplete");
    }

    let ConnectedTcpSocket { iface, sockets, handle } = tcp_socket;

    let mut _loop_ctr = 0;
    let mut state = HttpState::Requesting;
    let mut packet_byte_buffer: Vec<u8> = Vec::new();
    let mut packet_content_length: Option<usize> = None;
    let mut packet_header_length: Option<usize> = None;

    let startup_time = hpet_ticks!();

    // in the loop below, we do the actual work of sending the request and receiving the response 
    loop { 
        _loop_ctr += 1;

        let packet_io_occurred = poll_iface(&iface, sockets, startup_time)?;

        // check for timeout, only if no socket activity occurred
        if !packet_io_occurred {
            if let Some(timeout) = timeout_millis {
                if millis_since(startup_time)? > timeout {
                    error!("http_client: timed out after {} ms, in state {:?}", timeout, state);
                    return Err("http_client: timed out");
                }
            }
        }

        let mut socket = sockets.get::<TcpSocket>(*handle);

        state = match state {
            HttpState::Requesting if socket.can_send() => {
                debug!("http_client: sending HTTP request: {:?}", request);
                socket.send_slice(request.as_ref()).expect("http_client: cannot send request");
                HttpState::ReceivingResponse
            }

            HttpState::ReceivingResponse if socket.can_recv() => {
                // By default, we stay in the receiving state.
                // This is changed later if we end up receiving the entire packet.
                let mut new_state = HttpState::ReceivingResponse;

                let recv_result = socket.recv(|data| {
                    debug!("http_client: {} bytes on the recv buffer: \n{}",
                        data.len(),
                        unsafe {str::from_utf8_unchecked(data)}
                    );

                    // Eagerly append ALL of the received data onto the end of our packet slice, 
                    // which is necessary to attempt to parse it as an HTTP response.
                    // Later, we can remove bytes towards the end if we ended up appending too many bytes,
                    // e.g., we received more than enough bytes and some of them were for the next packet.
                    let orig_length = packet_byte_buffer.len();
                    packet_byte_buffer.extend_from_slice(data);

                    let bytes_popped_off = {
                        // Check to see if we've received the full HTTP response:
                        // First, by checking whether we have received all of the headers (and can properly parse them)
                        // Second, by getting the content length header and seeing if we've received the full content (in num bytes)
                        let mut headers = [httparse::EMPTY_HEADER; 64];
                        let mut response = httparse::Response::new(&mut headers);
                        let parsed_response = response.parse(&packet_byte_buffer);
                        debug!("http_client: Result {:?} from parsing HTTP Response: {:?}", parsed_response, response);

                        match parsed_response {
                            Ok(httparse::Status::Partial) => {
                                trace!("http_client: received partial HTTP response...");
                                // we haven't received all of the HTTP header bytes yet, 
                                // so pop off all of the bytes from the recv buffer into our packet
                                data.len()
                            }

                            Ok(httparse::Status::Complete(total_header_len)) => {
                                packet_header_length = Some(total_header_len);
                                trace!("http_client: received all headers in the HTTP response, len {}", total_header_len);

                                // Here: when we've received all headers, we may or may not be done receiving the full response.
                                // If there is a "Content-Length" header present, we can use that to see if all the bytes are received.
                                // If there is no such header, then there must be a "Connection: close" header, indicating that the response is complete.
                                // If neither headers exist, then there has been an unexpected problem, and we should return an error.

                                let content_length_result = response.headers.iter()
                                    .filter(|h| h.name == "Content-Length")
                                    .next()
                                    .ok_or("couldn't find \"Content-Length\" header")
                                    .and_then(|header| core::str::from_utf8(header.value)
                                        .map_err(|_e| "failed to convert content-length value to UTF-8 string")
                                    )
                                    .and_then(|s| s.parse::<usize>()
                                        .map_err(|_e| "failed to parse content-length header as usize")
                                    );

                                match content_length_result { 
                                    Ok(content_length) => {
                                        debug!("http_client: packet_byte_buffer len: {}, content_length: {}, header_len: {} (_loop_ctr: {})", 
                                            packet_byte_buffer.len(), content_length, total_header_len, _loop_ctr
                                        );
                                        packet_content_length = Some(content_length);
                                        // the total num of bytes that we want is the length of all the headers + the content
                                        let expected_length = total_header_len + content_length;
                                        if packet_byte_buffer.len() < expected_length {
                                            // here: we haven't gotten all of the content bytes yet, so we pop off all of the bytes received so far
                                            data.len()
                                        } else {
                                            // here: we *have* received all of the content, so the full response is ready
                                            debug!("http_client: HTTP response fully received. (_loop_ctr: {})", _loop_ctr);
                                            new_state = HttpState::Responded;
                                            // we pop off the exact number of bytes that make up the rest of the content,
                                            // leaving the rest on the recv buffer
                                            expected_length - orig_length
                                        } 
                                    }
                                    Err(_e) => {
                                        error!("http_client: {}", _e);
                                        // upon error, return 0, which instructs the recv() method to pop off no bytes from the recv buffer
                                        0
                                    }
                                }
                            }

                            Err(_e) => {
                                error!("http_client: Error parsing incoming html: {:?}", _e);
                                0
                            }
                        }
                    };

                    // Since we eagerly appended all of the received bytes onto this buffer, 
                    // we need to fix that up based on how many bytes we actually ended up popping off the recv buffer
                    packet_byte_buffer.truncate(orig_length + bytes_popped_off);

                    (bytes_popped_off, ())
                });
                new_state
            }

            HttpState::Responded => {
                debug!("http_client: received full {}-byte HTTP response (_loop_ctr: {}).", packet_byte_buffer.len(), _loop_ctr);
                break;
            }

            HttpState::ReceivingResponse if !socket.may_recv() => {
                error!("http_client: socket was closed prematurely before full reponse was received! (_loop_ctr: {})", _loop_ctr);
                return Err("socket was closed prematurely before full reponse was received!");
            }

            _ => { 
                // if _loop_ctr % 50000 == 0 {
                //     warn!("http_client: waiting in state {:?} for socket to send/recv ...", state);
                // }
                state
            }
        }
    }


    debug!("http_client: exiting HTTP state loop with state: {:?} (_loop_ctr: {})", state, _loop_ctr);

    Ok(HttpResponse {
        packet: packet_byte_buffer,
        header_length: packet_header_length.ok_or("BUG: received full HTTP response but couldn't determine packet header length")?,
    })
    
}
