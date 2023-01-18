//! Functions for creating and sending HTTP requests and receiving responses.
//! 

#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate smoltcp;
extern crate network_manager;
extern crate hpet;
extern crate httparse;
#[macro_use] extern crate smoltcp_helper;

use core::str;
use alloc::vec::Vec;
use alloc::string::String;
use hpet::get_hpet;
use smoltcp::{
    socket::{SocketSet, TcpSocket, SocketHandle},
};
use network_manager::{NetworkInterfaceRef};
use smoltcp_helper::{millis_since, poll_iface};

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

/// Checks to see if the provided HTTP request can be properly parsed, and returns true if so.
pub fn check_http_request(request_bytes: &[u8]) -> bool {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    request.parse(request_bytes).is_ok() && request_bytes.ends_with(b"\r\n\r\n")
}


/// TODO: create a proper HttpRequest type with header creation fields and actual verification
pub type HttpRequest = String;


/// An HttpResponse that has been fully received from a remote server.
/// 
/// TODO: revamp this structure to not store redundant data
pub struct HttpResponse {
    /// The actual array of raw bytes received from the server, 
    /// including all of the headers and body.
    pub packet: Vec<u8>,
    /// The length of all headers
    pub header_length: usize,
    /// The status code, e.g., 200, 404
    pub status_code: u16,
    /// The reason, e.g., "OK", "File not found"
    pub reason: String,
}
impl HttpResponse {
    pub fn header_bytes(&self) -> &[u8] {
        &self.packet[0 .. self.header_length]
    }

    fn content(&self) -> &[u8] {
        &self.packet[self.header_length ..]
    }

    /// Returns the content of this `HttpResponse` as a `Result`, 
    /// in which `Ok(content)` is returned if the status code is 200 (Ok),
    /// and `Err((status_code, reason))` is returned otherwise.
    pub fn as_result(&self) -> Result<&[u8], (u16, &str)> {
        if self.status_code == 200 {
            Ok(self.content())
        } else {
            Err((self.status_code, &self.reason))
        }
    }

    /// A convenience function that just returns a standard Err `&str`.
    pub fn as_result_err_str(&self) -> Result<&[u8], &'static str> {
        self.as_result().map_err(|_e| {
            error!("HttpResponse: error code {}, reason {:?}", _e.0, _e.1);
            "HttpResponse had an error status code (not Ok 200)"
        })
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
            iface,
            sockets,
            handle: tcp_socket_handle,
        })
    }
}

/// Sends the given HTTP request over the network via the given `socket` on the given `interface`,
/// waits to receive a full HTTP response from the remote endpoint, 
/// and then returns that full response, or an error if the response wasn't fully received properly.
/// 
/// # Arguments
/// * `request`: the HTTP request to be sent via the connected socket.
/// * `tcp_socket`: the connected TCP socket that will be used to send the HTTP request and receive the response.
/// * `inactivity_timeout_millis`: the timeout in milliseconds that limits how long this function will wait during periods of inactivity. 
///    This is not a timeout that bounds the total execution time of this function; the timer is reset when a packet is received. 
///    For example, a value of `5000` means that the function will give up if more than 5 seconds elapses without any packets being received.
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
    let mut packet_byte_buffer:   Vec<u8> = Vec::new();
    let mut packet_header_length: Option<usize> = None;
    let mut response_status_code: Option<u16> = None;
    let mut response_reason:      Option<String> = None;

    let startup_time = hpet_ticks!();
    let mut latest_packet_timestamp = startup_time;

    // in the loop below, we do the actual work of sending the request and receiving the response 
    loop { 
        _loop_ctr += 1;

        let _packet_io_occurred = poll_iface(iface, sockets, startup_time)?;

        // check if we have timed out
        if let Some(t) = timeout_millis {
            if millis_since(latest_packet_timestamp)? > t {
                error!("http_client: timed out after {} ms, in state {:?}", t, state);
                return Err("http_client: timed out");
            }
        }

        let mut socket = sockets.get::<TcpSocket>(*handle);

        state = match state {
            HttpState::Requesting if socket.can_send() => {
                debug!("http_client: sending HTTP request: {:?}", request);
                socket.send_slice(request.as_ref()).expect("http_client: cannot send request");
                latest_packet_timestamp = hpet_ticks!();
                HttpState::ReceivingResponse
            }

            HttpState::ReceivingResponse if socket.can_recv() => {
                // Stay in the receiving state for now; will be changed later if we receive the entire packet.
                let mut new_state = HttpState::ReceivingResponse;
                let orig_packet_length = packet_byte_buffer.len();

                let recv_result = socket.recv(|data| {
                    // Eagerly append ALL of the received data onto the end of our packet slice, 
                    // which is necessary to attempt to parse it as an HTTP response.
                    // Later, we can remove bytes towards the end if we ended up appending too many bytes,
                    // e.g., we received more than enough bytes and some of them were for the next packet.
                    packet_byte_buffer.extend_from_slice(data);

                    let bytes_popped_off = {
                        // Check to see if we've received the full HTTP response:
                        // First, by checking whether we have received all of the headers (and can properly parse them)
                        // Second, by getting the content length header and seeing if we've received the full content (in num bytes)
                        let mut headers = [httparse::EMPTY_HEADER; 64];
                        let mut response = httparse::Response::new(&mut headers);
                        match response.parse(&packet_byte_buffer) {
                            Ok(httparse::Status::Partial) => {
                                trace!("http_client: received partial HTTP response...");
                                // we haven't received all of the HTTP header bytes yet, 
                                // so pop off all of the bytes from the recv buffer into our packet
                                data.len()
                            }

                            Ok(httparse::Status::Complete(total_header_len)) => {
                                packet_header_length = Some(total_header_len);
                                response_status_code = response.code;
                                response_reason = response.reason.map(String::from);

                                // Here: we've received all headers, but we may not be done receiving the full response.
                                // If there is a "Content-Length" header present, we can use that to see if all the bytes are received.
                                // If there is no such header, then there might be a "Connection: close" header, indicating that the response is complete.
                                // If neither headers exist, then there has been an unexpected problem, and we should return an error.

                                if let Some(content_length_header) = response.headers.iter().find(|h| h.name == "Content-Length") {
                                    match str::from_utf8(content_length_header.value)
                                        .map_err(|_e| "failed to read Content-Length header value as UTF-8 string")
                                        .and_then(|s| s.parse::<usize>()
                                            .map_err(|_e| "failed to parse Content-Length header value as usize")
                                        )
                                    {
                                        Ok(content_length) => {
                                            // debug!("http_client: packet_byte_buffer len: {}, content_length: {}, header_len: {} (_loop_ctr: {})", 
                                            //     packet_byte_buffer.len(), content_length, total_header_len, _loop_ctr
                                            // );
                                            // the total num of bytes that we want is the length of all the headers + the content
                                            let expected_length = total_header_len + content_length;
                                            if packet_byte_buffer.len() < expected_length {
                                                // here: we haven't gotten all of the content bytes yet, so we pop off all of the bytes received so far
                                                data.len()
                                            } else {
                                                // here: we *have* received all of the content, so the full response is ready
                                                new_state = HttpState::Responded;
                                                // we pop off the exact number of bytes that make up the rest of the content,
                                                // leaving the rest on the recv buffer
                                                expected_length - orig_packet_length
                                            } 

                                        }
                                        Err(e) => {
                                            error!("http_client: {}", e);
                                            // upon error, return 0, which instructs the recv() method to pop off no bytes from the recv buffer
                                            0
                                        }
                                    }
                                } 
                                else if let Some(_connection_close_header) = response.headers.iter().find(|h| h.name == "Connection" && h.value == b"close") {
                                    // Here: the remote endpoint closed the connection, meaning that the entire response is on the recv buffer.
                                    new_state = HttpState::Responded;
                                    data.len()
                                }
                                else {
                                    error!("http_client: couldn't find Content-Length or Connection header, can't determine end of HTTP response");
                                    // upon error, return 0, which instructs the recv() method to pop off no bytes from the recv buffer
                                    0
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
                    packet_byte_buffer.truncate(orig_packet_length + bytes_popped_off);

                    (bytes_popped_off, ())
                });

                if let Err(_e) = recv_result {
                    error!("http_client: receive error on socket: {:?}", _e);
                    return Err("receive error on socket");
                }

                // if we just received another packet (the packet buffer changed size), then update the timeout deadline
                if orig_packet_length != packet_byte_buffer.len() {
                    latest_packet_timestamp = hpet_ticks!();
                }
                
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


    // debug!("http_client: exiting HTTP state loop with state: {:?} (_loop_ctr: {})", state, _loop_ctr);

    Ok(HttpResponse {
        packet: packet_byte_buffer,
        header_length: packet_header_length.ok_or("BUG: received full HTTP response but couldn't determine packet header length")?,
        status_code: response_status_code.ok_or("BUG: received full HTTP response but couldn't determine its status code")?,
        reason: response_reason.ok_or("BUG: received full HTTP response but couldn't determine its reason phrase")?,
    })
    
}
