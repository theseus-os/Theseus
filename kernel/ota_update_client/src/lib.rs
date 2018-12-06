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
extern crate spin;
extern crate dfqueue;
extern crate irq_safety;
extern crate acpi;
extern crate memory;
extern crate owning_ref;
extern crate spawn;
extern crate task;
extern crate httparse;


use core::convert::TryInto;
use core::str;
use core::fmt::Write;
use alloc::vec::Vec;
use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::prelude::SliceConcatExt;
use alloc::collections::BTreeMap;
use dfqueue::{DFQueue, DFQueueProducer};
use spin::Once;
use acpi::get_hpet;
use smoltcp::wire::{EthernetAddress, Ipv4Address, IpAddress, IpCidr};
use smoltcp::iface::{NeighborCache, EthernetInterfaceBuilder, Routes};
use smoltcp::socket::{SocketSet, TcpSocket, TcpSocketBuffer};
use smoltcp::time::Instant;
use e1000_smoltcp_device::E1000Device;
use network_interface_card::NetworkInterfaceCard;
use irq_safety::MutexIrqSafe;


/// The IP address of the update server.
/// This is currently the static IP of `kevin.recg.rice.edu`.
const DEFAULT_DESTINATION_IP_ADDR: [u8; 4] = [168, 7, 138, 84];

/// The TCP port on the update server that listens for update requests 
// const DEFAULT_DESTINATION_PORT: u16 = 60123;
const DEFAULT_DESTINATION_PORT: u16 = 8090;

/// A randomly chosen IP address that must be outside of the DHCP range.. // TODO FIXME: use DHCP to acquire IP
const DEFAULT_LOCAL_IP: [u8; 4] = [192, 168, 1, 252];

/// The TCP port on the this machine that can receive replies from the server.
const DEFAULT_LOCAL_PORT: u16 = 53145;

/// Standard home router address. // TODO FIXME: use DHCP to acquire gateway IP
const DEFAULT_LOCAL_GATEWAY_IP: [u8; 4] = [192, 168, 1, 1];



static MSG_QUEUE: Once<DFQueueProducer<String>> = Once::new();


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


/// Function to calculate time since a give time in ms
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
/// * `nic`: a reference to an initialized NIC, which must implement the `NetworkInterfaceCard` trait.
/// 
pub fn init<N>(nic: &'static MutexIrqSafe<N>) -> Result<(), &'static str> 
    where N: NetworkInterfaceCard
{
    Err("WIP does nothing right now")

    /*
    let startup_time = get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter();

    // initialize the message queue
    let msg_queue: DFQueue<String> = DFQueue::new();
    let msg_queue_consumer = msg_queue.into_consumer();
    let msg_queue_producer = msg_queue_consumer.obtain_producer();
    MSG_QUEUE.call_once(|| msg_queue_producer.obtain_producer());

    let dest_addr = IpAddress::v4(
        DEFAULT_DESTINATION_IP_ADDR[0],
        DEFAULT_DESTINATION_IP_ADDR[1],
        DEFAULT_DESTINATION_IP_ADDR[2],
        DEFAULT_DESTINATION_IP_ADDR[3],
    );
    let dest_port = DEFAULT_DESTINATION_PORT;

    // setup local ip address (this machine) // TODO FIXME: obtain an IP via DHCP
    let local_addr = [IpCidr::new(
        IpAddress::v4(
            DEFAULT_LOCAL_IP[0],
            DEFAULT_LOCAL_IP[1],
            DEFAULT_LOCAL_IP[2],
            DEFAULT_LOCAL_IP[3],
        ),
        24
    )];
    let local_port = DEFAULT_LOCAL_PORT;

    let neighbor_cache = NeighborCache::new(BTreeMap::new());
    
    let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 4096]);
    let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now
    let tcp_handle = sockets.add(tcp_socket);

    // Setup an interface/device that connects smoltcp to our e1000 driver
    let device = E1000Device::new(nic);
    let hardware_mac_addr = EthernetAddress(nic.lock().mac_address());
    // TODO FIXME: get the default gateway IP from DHCP
    let default_gateway = Ipv4Address::new(
        DEFAULT_LOCAL_GATEWAY_IP[0],
        DEFAULT_LOCAL_GATEWAY_IP[1],
        DEFAULT_LOCAL_GATEWAY_IP[2],
        DEFAULT_LOCAL_GATEWAY_IP[3],
    );
    let mut routes_storage = [None; 1];
    let mut routes = Routes::new(&mut routes_storage[..]);
    let _prev_default_gateway = routes.add_default_ipv4_route(default_gateway).map_err(|_e| {
        error!("ota_update_client: couldn't set default gateway IP address: {:?}", _e);
        "couldn't set default gateway IP address"
    })?;
    let mut iface = EthernetInterfaceBuilder::new(device)
        .ethernet_addr(hardware_mac_addr)
        .neighbor_cache(neighbor_cache)
        .ip_addrs(local_addr)
        .routes(routes)
        .finalize();

    let mut loop_ctr = 0;


    let mut state = HttpState::Connecting;

    let mut current_http_bytes: Vec<u8> = Vec::new();
    let mut current_http_response: Option<httparse::Response> = None;

    loop { 
        loop_ctr += 1;

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

        state = match state {
            HttpState::Connecting if !socket.is_active() => {
                debug!("ota_update_client: connecting...");
                socket.connect((dest_addr, dest_port), local_port).unwrap();
                debug!("ota_update_client: connected!");
                HttpState::Requesting
            }

            HttpState::Requesting if socket.can_send() => {
                let method = "GET ";
                let uri = "/a%23hello.o ";
                let version = "HTTP/1.1\r\n";
                let host = format_args!("Host: {}:{}\r\n", dest_addr, dest_port); // host
                let connection = "Connection: close\r\n";
                let http_request = format!("{}{}{}{}{}\r\n", 
                    method, 
                    uri, 
                    version,
                    host, 
                    connection
                );

                // sanity check the created HTTP request
                {
                    let mut headers = [httparse::EMPTY_HEADER; 64];
                    let mut request = httparse::Request::new(&mut headers);
                    if let Err(_e) = request.parse(http_request.as_bytes()) {
                        error!("ota_update_client: created improper HTTP request: {:?}", http_request);
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
                    debug!("ota_update_client: received data ({} bytes): \n{}",
                        data.len(),
                        unsafe {str::from_utf8_unchecked(data)}
                    );

                    // Eagerly append ALL the received data onto the end of our packet slice. 
                    // Later, we can remove bytes towards the end if we ended up appending too many bytes,
                    // e.g., we received more than enough bytes and some of them were for the next packet.
                    let orig_length = current_http_bytes.len();
                    current_http_bytes.extend_from_slice(data);

                    let mut headers = [httparse::EMPTY_HEADER; 64];
                    let mut response = httparse::Response::new(&mut headers);
                    let res = response.parse(&current_http_bytes);
                    debug!("ota_update_client: Result {:?} from parsing HTTP Response: {:?}", res, response);

                    // Check to see if we've received the full HTTP response:
                    // First, by checking whether we have received all of the headers 
                    // Second, by getting the content length header and seeing if we've received the full content (in num bytes)
                    match res {
                        Ok(httparse::Status::Partial) => {
                            trace!("ota_update_client: received partial HTTP response...");
                            // pop off all of the bytes from the recv buffer into our packet
                            (data.len(), ())
                        }
                        Ok(httparse::Status::Complete(total_header_len)) => {
                            trace!("ota_update_client: received all headers in the HTTP response, len {}", total_header_len);
                            match response.headers.iter()
                                .filter(|h| h.name == "Content-Length")
                                .next()
                                .ok_or("couldn't find \"Content-Length\" header")
                                .and_then(|header| core::str::from_utf8(header.value)
                                    .map_err(|_e| "failed to convert content-length value to UTF-8 string")
                                )
                                .and_then(|s| s.parse::<usize>()
                                    .map_err(|_e| "failed to parse content-length header as usize")
                                )
                            { 
                                Ok(content_length) => {
                                    debug!("ota_update_client: current_http_bytes len: {}, content_length: {}, header_len: {} (loop_ctr: {})", 
                                        current_http_bytes.len(), content_length, total_header_len, loop_ctr
                                    );
                                    // the total num of bytes that we want is the length of all the headers + the content
                                    let expected_length = total_header_len + content_length;
                                    if current_http_bytes.len() >= expected_length {
                                        current_http_bytes.truncate(expected_length);
                                        let bytes_popped = current_http_bytes.len() - orig_length;
                                        // set state to Responded if we've received all the content
                                        debug!("ota_update_client: HTTP response fully received. (loop_ctr: {})", loop_ctr);
                                        new_state = HttpState::Responded;
                                        (bytes_popped, ())
                                    } 
                                    else {
                                        // here: we haven't gotten all of the content bytes yet, so we pop off all of the bytes received so far
                                        (data.len(), ())
                                    }
                                }
                                Err(_e) => {
                                    error!("ota_update_client: {}", _e);
                                    // upon error, return 0, which instructs the recv() method to pop off 0 bytes from the recv buffer
                                    (0, ())
                                }
                            }
                        }

                        Err(_e) => {
                            error!("ota_update_client: Error parsing incoming html: {:?}", _e);
                            return (0, ());
                        }
                    }

                    
                });

                new_state
            }

            HttpState::Responded => {
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
        let _packets_were_sent_or_received = match iface.poll(&mut sockets, Instant::from_millis(timestamp)) {
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

    */
}
