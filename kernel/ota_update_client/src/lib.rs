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
const DEFAULT_DESTINATION_PORT: u16 = 60123;

/// A randomly chosen IP address that must be outside of the DHCP range.. // TODO FIXME: use DHCP to acquire IP
const DEFAULT_LOCAL_IP: [u8; 4] = [192, 168, 1, 252];

/// The TCP port on the this machine that can receive replies from the server.
const DEFAULT_LOCAL_PORT: u16 = 53132;

/// Standard home router address. // TODO FIXME: use DHCP to acquire gateway IP
const DEFAULT_LOCAL_GATEWAY_IP: [u8; 4] = [192, 168, 1, 1];



static MSG_QUEUE: Once<DFQueueProducer<String>> = Once::new();



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
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
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

    {
        let mut socket = sockets.get::<TcpSocket>(tcp_handle);
        socket.connect((dest_addr, dest_port), local_port).unwrap();
    }


    
    // /*  SIMPLE TCP TEST START

    let mut loop_ctr = 0;
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
    
    // END SIMPLE TCP TEST */

    
    
    
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
