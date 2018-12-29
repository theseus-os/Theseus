//! Functions to communicate with a network server that provides over-the-air live update functionality.
//! 

#![no_std]
#![feature(alloc)]
#![feature(try_from)]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
extern crate smoltcp;
extern crate e1000;
extern crate e1000_smoltcp_device;
extern crate network_interface_card;
extern crate network_manager;
extern crate spin;
extern crate dfqueue;
extern crate irq_safety;
extern crate acpi;
extern crate memory;
extern crate owning_ref;
extern crate spawn;
extern crate task;
extern crate httparse;
extern crate sha3;
extern crate percent_encoding;
extern crate rand;


use core::convert::TryInto;
use core::str;
use alloc::vec::Vec;
use spin::Once;
use acpi::get_hpet;
use smoltcp::{
    wire::IpAddress,
    socket::{SocketSet, TcpSocket, TcpSocketBuffer},
    time::Instant
};
use sha3::{Digest, Sha3_512};
use percent_encoding::{DEFAULT_ENCODE_SET, utf8_percent_encode};
use network_manager::{NetworkInterfaceRef};
use rand::{
    SeedableRng,
    RngCore,
    rngs::SmallRng
};


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
    /// The response has been received in full, including the entire content.
    Responded
}


/// A simple macro to get the current HPET clock ticks.
macro_rules! hpet_ticks {
    () => {
        get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter()
    };
}


/// Function to calculate time since a given time in ms
fn millis_since(start_time: u64) -> Result<u64, &'static str> {
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
fn poll_iface(iface: &NetworkInterfaceRef, sockets: &mut SocketSet, startup_time: u64) -> Result<bool, &'static str> {
    let timestamp: i64 = millis_since(startup_time)?
        .try_into()
        .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
    let packets_were_sent_or_received = match iface.lock().poll(sockets, Instant::from_millis(timestamp)) {
        Ok(b) => b,
        Err(err) => {
            warn!("ota_update_client: poll error: {}", err);
            false
        }
    };
    Ok(packets_were_sent_or_received)
}


/// Checks to see if the provided HTTP request can be properly parsed, and returns true if so.
fn check_http_request(request_bytes: &[u8]) -> bool {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    request.parse(request_bytes).is_ok() && request_bytes.ends_with(b"\r\n\r\n")
}



/// Initialize the network live update client, 
/// which spawns a new thread to handle live update requests and notifications. 
/// # Arguments
/// * `iface`: a reference to an initialized network interface
/// 
pub fn init(iface: NetworkInterfaceRef) -> Result<(), &'static str> {

    let startup_time = hpet_ticks!();

    let dest_addr = IpAddress::v4(
        DEFAULT_DESTINATION_IP_ADDR[0],
        DEFAULT_DESTINATION_IP_ADDR[1],
        DEFAULT_DESTINATION_IP_ADDR[2],
        DEFAULT_DESTINATION_IP_ADDR[3],
    );
    let dest_port = DEFAULT_DESTINATION_PORT;

    let mut rng = SmallRng::seed_from_u64(startup_time);
    let local_port = STARTING_FREE_PORT + rng.next_u32() as u16;

    let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now
    let tcp_handle = sockets.add(tcp_socket);

    let http_request = {
        let method = "GET";
        let uri = utf8_percent_encode("/a#hello.o", DEFAULT_ENCODE_SET);
        let version = "HTTP/1.1";
        let connection = "Connection: close";
        format!("{} {} {}\r\n{}\r\n{}\r\n\r\n", 
            method,
            uri,
            version,
            format_args!("Host: {}:{}", dest_addr, dest_port), // host
            connection
        )
    };

    if !check_http_request(http_request.as_bytes()) {
        error!("ota_update_client: created improper/incomplete HTTP request: {:?}.", http_request);
        return Err("ota_update_client: created improper/incomplete HTTP request");
    }


    {
        info!("ota_update_client: connecting from {}:{} to {}:{}",
            iface.lock().ip_addrs().get(0).map(|ip| format!("{}", ip)).unwrap_or_else(|| format!("ERROR")), 
            local_port, 
            dest_addr, 
            dest_port
        );
    }


    // first, attempt to connect the socket to the remote server
    let timeout_ms = 3000; // 3 second timeout
    let start = hpet_ticks!();
    
    if sockets.get::<TcpSocket>(tcp_handle).is_active() {
        warn!("ota_update_client: when connecting socket, it was already active...");
    } else {
        debug!("ota_update_client: connecting socket...");
        sockets.get::<TcpSocket>(tcp_handle).connect((dest_addr, dest_port), local_port).map_err(|_e| {
            error!("ota_update_client: failed to connect socket, error: {:?}", _e);
            "ota_update_client: failed to connect socket"
        })?;

        loop {
            let _packet_io_occurred = poll_iface(&iface, &mut sockets, startup_time)?;
            
            // if the socket actually connected, it should be able to send/recv
            let mut socket = sockets.get::<TcpSocket>(tcp_handle);
            if socket.may_send() && socket.may_recv() {
                break;
            }

            // check to make sure we haven't timed out
            if millis_since(start)? > timeout_ms {
                error!("ota_update_client: failed to connect to socket, timed out after {} ms", timeout_ms);
                return Err("ota_update_client: failed to connect to socket, timed out.");
            }
        }
    }

    debug!("ota_update_client: socket connected successfully!");

    let mut loop_ctr = 0;
    let mut state = HttpState::Requesting;
    let mut current_packet_byte_buffer: Vec<u8> = Vec::new();
    let mut current_packet_content_length: Option<usize> = None;
    let mut current_packet_header_length: Option<usize> = None;

    loop { 
        loop_ctr += 1;

        let _packet_io_occurred = poll_iface(&iface, &mut sockets, startup_time)?;

        let mut socket = sockets.get::<TcpSocket>(tcp_handle);

        state = match state {
            HttpState::Requesting if socket.can_send() => {
                debug!("ota_update_client: sending HTTP request: {:?}", http_request);
                socket.send_slice(http_request.as_ref()).expect("ota_update_client: cannot send request");
                HttpState::ReceivingResponse
            }

            HttpState::ReceivingResponse if socket.can_recv() => {
                // By default, we stay in the receiving state.
                // This is changed later if we end up receiving the entire packet.
                let mut new_state = HttpState::ReceivingResponse;

                let recv_result = socket.recv(|data| {
                    debug!("ota_update_client: {} bytes on the recv buffer: \n{}",
                        data.len(),
                        unsafe {str::from_utf8_unchecked(data)}
                    );

                    // Eagerly append ALL of the received data onto the end of our packet slice, 
                    // which is necessary to attempt to parse it as an HTTP response.
                    // Later, we can remove bytes towards the end if we ended up appending too many bytes,
                    // e.g., we received more than enough bytes and some of them were for the next packet.
                    let orig_length = current_packet_byte_buffer.len();
                    current_packet_byte_buffer.extend_from_slice(data);

                    let bytes_popped_off = {
                        // Check to see if we've received the full HTTP response:
                        // First, by checking whether we have received all of the headers (and can properly parse them)
                        // Second, by getting the content length header and seeing if we've received the full content (in num bytes)
                        let mut headers = [httparse::EMPTY_HEADER; 64];
                        let mut response = httparse::Response::new(&mut headers);
                        let parsed_response = response.parse(&current_packet_byte_buffer);
                        debug!("ota_update_client: Result {:?} from parsing HTTP Response: {:?}", parsed_response, response);

                        match parsed_response {
                            Ok(httparse::Status::Partial) => {
                                trace!("ota_update_client: received partial HTTP response...");
                                // we haven't received all of the HTTP header bytes yet, 
                                // so pop off all of the bytes from the recv buffer into our packet
                                data.len()
                            }

                            Ok(httparse::Status::Complete(total_header_len)) => {
                                current_packet_header_length = Some(total_header_len);
                                trace!("ota_update_client: received all headers in the HTTP response, len {}", total_header_len);
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
                                        debug!("ota_update_client: current_packet_byte_buffer len: {}, content_length: {}, header_len: {} (loop_ctr: {})", 
                                            current_packet_byte_buffer.len(), content_length, total_header_len, loop_ctr
                                        );
                                        current_packet_content_length = Some(content_length);
                                        // the total num of bytes that we want is the length of all the headers + the content
                                        let expected_length = total_header_len + content_length;
                                        if current_packet_byte_buffer.len() < expected_length {
                                            // here: we haven't gotten all of the content bytes yet, so we pop off all of the bytes received so far
                                            data.len()
                                        } else {
                                            // here: we *have* received all of the content, so the full response is ready
                                            debug!("ota_update_client: HTTP response fully received. (loop_ctr: {})", loop_ctr);
                                            new_state = HttpState::Responded;
                                            // we pop off the exact number of bytes that make up the rest of the content,
                                            // leaving the rest on the recv buffer
                                            expected_length - orig_length
                                        } 
                                    }
                                    Err(_e) => {
                                        error!("ota_update_client: {}", _e);
                                        // upon error, return 0, which instructs the recv() method to pop off no bytes from the recv buffer
                                        0
                                    }
                                }
                            }

                            Err(_e) => {
                                error!("ota_update_client: Error parsing incoming html: {:?}", _e);
                                0
                            }
                        }
                    };

                    // Since we eagerly appended all of the received bytes onto this buffer, 
                    // we need to fix that up based on how many bytes we actually ended up popping off the recv buffer
                    current_packet_byte_buffer.truncate(orig_length + bytes_popped_off);

                    (bytes_popped_off, ())
                });
                new_state
            }

            HttpState::Responded => {
                debug!("ota_update_client: received full {}-byte HTTP response (loop_ctr: {}): \n{}", 
                    current_packet_byte_buffer.len(), loop_ctr, unsafe { str::from_utf8_unchecked(&current_packet_byte_buffer) });
                break;
            }

            HttpState::ReceivingResponse if !socket.may_recv() => {
                warn!("ota_update_client: socket was closed prematurely before full reponse was received! (loop_ctr: {})", loop_ctr);
                break;
            }

            _ => { 
                if loop_ctr % 50000 == 0 {
                    debug!("ota_update_client: waiting in state {:?} for socket to send/recv ...", state);
                }
                state
            }
        }
    }


    debug!("ota_update_client: exiting HTTP state loop with state: {:?} (loop_ctr: {})", state, loop_ctr);

    // calculate the sha3-512 hash of the HTTP response body (excluding headers)
    let mut hasher = Sha3_512::new();
    hasher.input(&current_packet_byte_buffer[current_packet_header_length.unwrap() ..]);
    let result = hasher.result();
    info!("ota_update_client: sha3-512 hash of downloaded file: {:x}", result);


    let mut issued_close = false;
    loop {
        loop_ctr += 1;

        let _packet_io_occurred = poll_iface(&iface, &mut sockets, startup_time)?;

        let mut socket = sockets.get::<TcpSocket>(tcp_handle);
        if !issued_close {
            debug!("ota_update_client: socket state is {:?}", socket.state());

            debug!("ota_update_client: closing socket...");
            socket.close();
            debug!("ota_update_client: socket state (after close) is now {:?}", socket.state());
            issued_close = true;
        }

        if loop_ctr % 50000 == 0 {
            debug!("ota_update_client: socket state (looping) is now {:?}", socket.state());
        }

    }

    Ok(())
    
}
