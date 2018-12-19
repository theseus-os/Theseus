//! A sample implementation of a network application 
//! that uses the smoltcp networking stack and our e1000 driver
//! to communicate with another machine on the network.
//! 
//! In the future, this will be the crate that contains the 
//! over-the-air live update client-side functionality 
//! that communicates with the build server.
//! 

#![no_std]
#![feature(alloc)]
#![feature(try_from)]

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
// extern crate logger;
extern crate memory;
extern crate owning_ref;

use core::convert::TryInto;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::collections::BTreeMap;
use dfqueue::{DFQueue, DFQueueProducer, DFQueueConsumer};
use spin::Once;
// use core::fmt;
use acpi::get_hpet;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr};
use smoltcp::iface::{NeighborCache, EthernetInterfaceBuilder};
use smoltcp::socket::SocketSet;
use smoltcp::socket::{UdpSocket, UdpSocketBuffer, UdpPacketMetadata};
use smoltcp::wire::IpEndpoint;
use e1000_smoltcp_device::E1000Device;
use network_interface_card::NetworkInterfaceCard;
use irq_safety::MutexIrqSafe;


static MSG_QUEUE: Once<DFQueueProducer<String>> = Once::new();


/// Initialize the network server, which spawns a new thread 
/// to handle transmitting and receiving of packets in a loop.
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
    
    let skb_size = 8 * 1024; // 8KiB, randomly chosen

    // For UDP
    let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; skb_size]);
    let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; skb_size]);
    let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
    
    // For TCP - not in use yet
    // let tcp1_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    // let tcp1_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    // let tcp1_socket = TcpSocket::new(tcp1_rx_buffer, tcp1_tx_buffer);

    // let tcp2_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    // let tcp2_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    // let tcp2_socket = TcpSocket::new(tcp2_rx_buffer, tcp2_tx_buffer);

    let neighbor_cache = NeighborCache::new(BTreeMap::new());

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now

    let udp_handle = sockets.add(udp_socket);
    // let tcp1_handle = sockets.add(tcp1_socket);
    // let tcp2_handle = sockets.add(tcp2_socket);

    // create a device that connects the smoltcp data link layer to our e1000 driver
    let device: E1000Device<N> = E1000Device::new(nic);

    // setup destination address and port
    let dest_addr = IpAddress::v4(192, 168, 69, 100); // the default gateway (router?)
    let dest_port = 5901;

    // setup host ip address (this machine)
    let local_addr = IpCidr::new(IpAddress::v4(192, 168, 69, 1), 24);
    let local_port = 6969;

    // Initializing the Ethernet interface
    let hardware_mac_addr = EthernetAddress(nic.lock().mac_address());
    let mut iface = EthernetInterfaceBuilder::new(device)
        .ethernet_addr(hardware_mac_addr)
        .neighbor_cache(neighbor_cache)
        .ip_addrs([local_addr])
        .finalize();

    // bind the udp socket to a local port
    {
        let mut udp_socket = sockets.get::<UdpSocket>(udp_handle);
        if let Err(_e) = udp_socket.bind(local_port) {
            error!("network_test::init(): error binding UDP socket: {:?}", _e);
        }
    }

    let dest_endpoint = IpEndpoint {
        addr: dest_addr,
        port: dest_port,
    };

    #[cfg(test_network)] {
        send_msg_udp("FIRST UDP TEST MESSAGE")?;
        send_msg_udp("SECOND UDP TEST MESSAGE")?;
    }

    // the main loop for processing network transmit/receive operations, should be run as its own Task.
    loop {
        // poll the smoltcp ethernet interface (i.e., flush tx/rx)
        let timestamp: i64 = millis_since(startup_time)?
            .try_into()
            .map_err(|_e| "millis_since() u64 timestamp was larger than i64")?;
        let _packets_were_sent_or_received = match iface.poll(&mut sockets, Instant::from_millis(timestamp)) {
            Ok(b) => b,
            Err(err) => {
                warn!("network_test::init(): poll error: {}", err);
                false  // continue;
            }
        };  

        {
            let mut udp_socket = sockets.get::<UdpSocket>(udp_handle);

            // Receiving packets
            match udp_socket.recv() {
                Ok((data, endpoint)) => {
                    warn!("Received UDP packet from endpoint {:?}:\n{:?}", endpoint, data);
                }
                _ => { }
            }

            // Sending packets
            match msg_queue_consumer.peek() {
                Some(element) => {
                    debug!("network_test::init(): about to send UDP packet {:?}", &*element);
                    debug!("network_test::init(): can_send: {:?}, can_recv: {:?}", udp_socket.can_send(), udp_socket.can_recv());
                    if udp_socket.can_send() {
                        debug!("network_test::init(): sending UDP packet...");
                        if let Err(_e) = udp_socket.send_slice(element.as_bytes(), dest_endpoint) {
                            error!("network_test::init(): UDP sending error {}", _e);
                            continue;
                        }
                        element.mark_completed();
                    } else {
                        warn!("network_test::init(): UDP socket wasn't ready to send, skipping....")
                    }
                }
                _ => { }
            }
        }
    }
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

// /// Function to send debug messages using UDP to configured destination
// /// Used to mirror debug messages 
// pub fn send_log_msg_udp(_color: &logger::LogColor, prefix: &'static str, msg: fmt::Arguments){
//     if let Some(producer) = MSG_QUEUE.try(){
//         let s = format!("{}{}", prefix, msg);
//         producer.enqueue(s.to_string());  
//     }
// }

/// Queue up a message to be sent over the network (UDP socket) to the predetermined destination
pub fn send_msg_udp<T: ToString>(msg: T) -> Result<(), &'static str> {
    MSG_QUEUE.try()
        .ok_or("the network's UDP message queue was not yet initialized")?
        .enqueue(msg.to_string());

    Ok(())
}
