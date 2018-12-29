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
    /// The socket is not yet connected.
    Connecting,
    /// The socket is connected, but the HTTP request has not yet been sent.
    Requesting,
    /// The HTTP request has been sent, but the response has not yet been fully received.
    ReceivingResponse,
    /// The response has been received in full, including the entire content.
    Responded
}


/// Function to calculate time since a given time in ms
fn millis_since(start_time: u64) -> Result<u64, &'static str> {
    let hpet_guard = get_hpet();
    let hpet = hpet_guard.as_ref().ok_or("couldn't get HPET")?;
    let end_time: u64 = hpet.get_counter();
    let hpet_freq: u64 = hpet.counter_period_femtoseconds() as u64;
    // Convert to ms
    let diff = (end_time - start_time) * hpet_freq / 1_000_000_000_000;
    Ok(diff)
}



/// Initialize the network live update client, 
/// which spawns a new thread to handle live update requests and notifications. 
/// # Arguments
/// * `iface`: a reference to an initialized network interface
/// 
pub fn init(iface: NetworkInterfaceRef) -> Result<(), &'static str> {

    let startup_time = get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter();

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

    {
        info!("ota_update_client: connecting from {}:{} to {}:{}",
            iface.lock().ip_addrs().get(0).map(|ip| format!("{}", ip)).unwrap_or_else(|| format!("ERROR")), 
            local_port, 
            dest_addr, 
            dest_port
        );
    }


    let mut loop_ctr = 0;
    let mut state = HttpState::Connecting;
    let mut current_packet_byte_buffer: Vec<u8> = Vec::new();
    let mut current_packet_content_length: Option<usize> = None;
    let mut current_packet_header_length: Option<usize> = None;

    loop { 
        loop_ctr += 1;

        // poll the smoltcp ethernet interface (i.e., flush tx/rx)
        {
            let timestamp: i64 = millis_since(startup_time)?
                .try_into()
                .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
            let _packets_were_sent_or_received = match iface.lock().flush(&mut sockets, Instant::from_millis(timestamp)) {
                Ok(b) => b,
                Err(err) => {
                    warn!("ota_update_client: poll error: {}", err);
                    false  // continue;
                }
            };
        }

        let mut socket = sockets.get::<TcpSocket>(tcp_handle);

        state = match state {
            HttpState::Connecting if !socket.is_active() => {
                debug!("ota_update_client: connecting...");
                socket.connect((dest_addr, dest_port), local_port).unwrap();
                debug!("ota_update_client: connected!");
                HttpState::Requesting
            }

            HttpState::Requesting if socket.can_send() => {
                let method = "GET";
                let uri = utf8_percent_encode("/a#hello.o", DEFAULT_ENCODE_SET);
                let version = "HTTP/1.1";
                let connection = "Connection: close";
                let http_request = format!("{} {} {}\r\n{}\r\n{}\r\n\r\n", 
                    method,
                    uri,
                    version,
                    format_args!("Host: {}:{}", dest_addr, dest_port), // host
                    connection
                );

                // sanity check the created HTTP request
                {
                    let mut headers = [httparse::EMPTY_HEADER; 64];
                    let mut request = httparse::Request::new(&mut headers);
                    if let Err(_e) = request.parse(http_request.as_bytes()) {
                        error!("ota_update_client: created improper HTTP request: {:?}. Error: {:?}", http_request, _e);
                        break;
                    }
                }

                debug!("ota_update_client: sending request: {:?}", http_request);
                socket.send_slice(http_request.as_ref()).expect("ota_update_client: cannot send request");
                HttpState::ReceivingResponse
            }

            HttpState::ReceivingResponse if socket.can_recv() => {
                // By default, we stay in the receiving state.
                // This is changed later if we end up receiving the entire packet.1
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

    // calculate the sha3-512 hash
    let mut hasher = Sha3_512::new();
    hasher.input(&current_packet_byte_buffer[current_packet_header_length.unwrap() ..]);
    let result = hasher.result();
    info!("ota_update_client: sha3-512 hash of downloaded file: {:x}", result);

    
    /* SIMPLE TCP TEST START

    {
        let mut socket = sockets.get::<TcpSocket>(tcp_handle);
        socket.connect((dest_addr, dest_port), local_port).unwrap();
    }

    let mut msg_ctr = 0;
    let mut done_sending = false;
    loop { 
        // poll the smoltcp ethernet interface (i.e., flush tx/rx)
        let timestamp: i64 = millis_since(startup_time)?
            .try_into()
            .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
        let _packets_were_sent_or_received = match iface.poll(&mut sockets, Instant::from_millis(timestamp)) {
            Ok(b) => b,
            Err(err) => {
                warn!("ota_update_client: poll error: {}", err);
                false  // continue;
            }
        };  

        let mut socket = sockets.get::<TcpSocket>(tcp_handle);
        if !done_sending {
            if socket.can_send() {
                if msg_ctr % 2 == 0 {
                    let msg = format!("test message {}", msg_ctr);
                    debug!("ota_update_client: sending msg {:?} ...", msg);
                    write!(socket, "{}", msg).unwrap();
                    debug!("ota_update_client: Sent msg {:?}", msg);
                    msg_ctr += 1;
                }

                if msg_ctr > 8 {
                    info!("\n\nota_update_client: completed sending messages!\n\n");
                    done_sending = true;
                }
            }
            else {
                if loop_ctr % 50000 == 0 {
                    debug!("ota_update_client: waiting for socket can_send...");
                }
            }
        }


        if socket.can_recv() {
            if msg_ctr % 2 == 1 {
                debug!("ota_update_client: waiting to receive msg {}", msg_ctr);
                let recv_data = socket.recv(|buf| {
                    let msg = String::from(str::from_utf8(&buf).unwrap());
                    (msg.as_bytes().len(), msg)
                }).unwrap();
                debug!("ota_update_client: received msg: {}", recv_data);
                msg_ctr += 1;

                if recv_data == "Echo msg: test message 8" {
                    info!("\n\nota_update_client: completed receiving messages!\n\n");
                    break;
                }
            }
        }
        else {
            if loop_ctr % 50000 == 0 {
                debug!("ota_update_client: waiting for socket can_recv...");
            }
        }

        loop_ctr += 1;
    }
    END SIMPLE TCP TEST */


    let mut issued_close = false;
    loop {
        loop_ctr += 1;

        // poll the smoltcp ethernet interface (i.e., flush tx/rx)
        let timestamp: i64 = millis_since(startup_time)?
            .try_into()
            .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
        let _packets_were_sent_or_received = match iface.lock().flush(&mut sockets, Instant::from_millis(timestamp)) {
            Ok(b) => b,
            Err(err) => {
                warn!("ota_update_client: poll error: {}", err);
                false  // continue;
            }
        };


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
    

    
    
    
    /*

    let mut tcp_active = false;
    let mut initial_val_send = false;
    let mut number_of_items = 4;
    let mut send_items = 0;
    let mut received_items = 0;

    let mut array: Vec<String> = vec![String::new(); 512];
    let mut break_1 = 0;

    loop {
        // poll the smoltcp ethernet interface (i.e., flush tx/rx)
        let timestamp: i64 = millis_since(startup_time)?
            .try_into()
            .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
        let _packets_were_sent_or_received = match iface.poll(&mut sockets, Instant::from_millis(timestamp)) {
            Ok(b) => b,
            Err(err) => {
                warn!("ota_update_client: poll error: {}", err);
                false  // continue;
            }
        };  

        {
            let mut socket = sockets.get::<TcpSocket>(tcp_handle);
            if socket.is_active() && !tcp_active {
                debug!("ota_update_client: connected");
            } else if !socket.is_active() && tcp_active {
                debug!("ota_update_client: disconnected");
                return Err("tcp socket disconnected.");
            }
            tcp_active = socket.is_active();

            if socket.may_send() {
                if socket.can_send() && send_items == 0 {
                    debug!("ota_update_client: tcp:6969 send greeting");
                    write!(socket, "a#terminal_print.o").unwrap();
                    debug!("ota_update_client: Send 0");
                    send_items = send_items + 1;
                }
                if socket.can_send() && send_items == 1 {
                    write!(socket, "a#test_panic.o").unwrap();
                    debug!("ota_update_client: Send 1");
                    send_items = send_items + 1;
                }
                if socket.can_send() && send_items == 2 {
                    write!(socket, "k#acpi-b3db4f72ccdc307b.o").unwrap();
                    debug!("ota_update_client: Send 2");
                    send_items = send_items + 1;
                }
                if socket.can_send() && send_items == 3 {
                    write!(socket, "k#ap_start-5e82a1be3db78c92.o").unwrap();
                    debug!("ota_update_client: Send 3");
                    send_items = send_items + 1;
                }
                if socket.can_send() && send_items == 4 {
                    write!(socket, "Done.n").unwrap(); //This word is needed to end the sending of modules
                    debug!("ota_update_client: Send 4 -- Done.");
                    send_items = 100;
                }
            }

            if socket.can_recv() && send_items == 100 {
                let data = socket.recv(|data| {
                    let mut data = data.to_owned();
                    if data.len() > 0 {
                        //debug!("ota_update_client: recv data: {:?}",
                        //str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                        data = data.split(|&b| b == b'\n').collect::<Vec<_>>().concat();
                        //data.reverse();
                        data.extend(b"\n");
                        let mut mystring =
                            str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)");
                        //debug!("ota_update_client: {}",mystring);
                        let v: Vec<&str> = mystring.split(':').collect();
                        //debug!("ota_update_client: {}",v[0]);
                        if let "Done.." = &*v[0] {
                            debug!("ota_update_client: Module Receive Completed");
                            //debug!("ota_update_client: close");
                            //socket.close();
                            break_1 = 1;
                        } else {
                            debug!("ota_update_client: {}", v[0]);
                        }
                        //debug!("ota_update_client: recv data: {:?}",
                        //str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                        received_items = received_items + 1;
                    }
                    (data.len(), data)
                })
                .unwrap();
            }
            if break_1 == 1 {
                debug!("ota_update_client: closing socket after completion.");
                socket.close();
                return Err("ota_update_client: closing socket after completion.");
            }
        }
    }

    */
}
