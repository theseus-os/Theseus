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
extern crate task;


use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use dfqueue::{DFQueue, DFQueueProducer};
use spin::Once;
// use core::fmt;
use acpi::get_hpet;
use smoltcp::wire::{EthernetAddress, IpAddress};
use smoltcp::iface::{ArpCache, SliceArpCache, EthernetInterface};
use smoltcp::socket::{AsSocket, SocketSet}; 
use smoltcp::socket::{UdpSocket, UdpSocketBuffer, UdpPacketBuffer};
use smoltcp::wire::IpEndpoint;
use smoltcp::phy::Device;
use e1000_smoltcp_device::E1000Device;
use network_interface_card::NetworkInterfaceCard;
use task::TaskRef;
use irq_safety::MutexIrqSafe;


static MSG_QUEUE: Once<DFQueueProducer<String>> = Once::new();


/// Initialize the network server, which spawns a new thread 
/// to handle transmitting and receiving of packets in a loop.
/// # Arguments
/// * `nic`: a reference to an initialized NIC, which must implement
///   the `NetworkInterfaceCard` trait and smoltcp's `Device` trait.
/// 
/// Returns a tuple including the following (in order):
/// * a reference to the newly-spawned network task
/// * a queue producer that can be used to enqueue messages to be sent out over the network
pub fn init<N>(nic: &MutexIrqSafe<N>) -> Result<(TaskRef, DFQueueProducer<String>), &'static str> 
    where N: NetworkInterfaceCard + Device
{
    let startup_time = get_hpet().as_ref().ok_or("coudln't get HPET timer")?.get_counter();
    let skb_size = 8 * 1024; // 8KiB, randomly chosen

    // For UDP
    let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; skb_size])]);
    let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; skb_size])]);
    let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
    
    // For TCP - not in use yet
    // let tcp1_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    // let tcp1_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    // let tcp1_socket = TcpSocket::new(tcp1_rx_buffer, tcp1_tx_buffer);

    // let tcp2_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    // let tcp2_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    // let tcp2_socket = TcpSocket::new(tcp2_rx_buffer, tcp2_tx_buffer);

    let arp_cache = SliceArpCache::new(vec![Default::default(); 8]);

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now

    let udp_handle = sockets.add(udp_socket);
    // let tcp1_handle = sockets.add(tcp1_socket);
    // let tcp2_handle = sockets.add(tcp2_socket);

    // create a device that connects the smoltcp data link layer to our e1000 driver
    let device = E1000Device::new(nic);

    // setup destination address and port
    let dest_addr = IpAddress::v4(192, 168, 69, 100); // the default gateway (router?)
    let dest_port = 5901;

    // setup host ip address (this machine)
    let local_addr = [IpAddress::v4(192, 168, 69, 1)];
    let local_port = 6969;

    // Initializing the Ethernet interface
    let hardware_mac_addr = EthernetAddress(nic.lock().mac_address());
    let mut iface = EthernetInterface::new(
        Box::new(device), 
        Box::new(arp_cache) as Box<ArpCache>,
        hardware_mac_addr, 
        local_addr
    );  


    let dest_endpoint = IpEndpoint {
        addr: dest_addr,
        port: dest_port,
    };

    // code to initialize the DFQ
    let udpserver_dfq: DFQueue<String> = DFQueue::new();
    let udpserver_consumer = udpserver_dfq.into_consumer();
    let _udpserver_producer = udpserver_consumer.obtain_producer();
    MSG_QUEUE.call_once(|| {
       udpserver_consumer.obtain_producer()
    });

    #[cfg(test_network)] {
        send_msg_udp("FIRST UDP TEST MESSAGE");
        send_msg_udp("SECOND UDP TEST MESSAGE");
    }

    // bind the udp socket to a local port
    {
        let udp_socket: &mut UdpSocket = sockets.get_mut(udp_handle).as_socket();
        if let Err(_e) = udp_socket.bind(local_port) {
            error!("server_init(): error binding UDP socket: {:?}", _e);
        }
    }

    // Main loop for the server
    loop {

        {
            let udp_socket: &mut UdpSocket = sockets.get_mut(udp_handle).as_socket();

            // Receiving packets
            match udp_socket.recv() {
                Ok((data, endpoint)) => {
                    warn!("Received UDP packet from endpoint {:?}:\n{:?}", endpoint, data);
                }
                _ => { }
            }

            // Sending packets
            match udpserver_consumer.peek() {
                Some(element) => {
                    debug!("server_init(): about to send UDP packet {:?}", &*element);
                    debug!("server_init(): can_send: {:?}, can_recv: {:?}", udp_socket.can_send(), udp_socket.can_recv());
                    if udp_socket.can_send() {
                        debug!("server_init(): sending UDP packet...");
                        if let Err(_e) = udp_socket.send_slice(element.as_bytes(), dest_endpoint) {
                            error!("server_init(): UDP sending error {}", _e);
                            continue;
                        }
                        element.mark_completed();
                    } else {
                        warn!("server_init(): UDP socket wasn't ready to send, skipping....")
                    }
                }
                _ => { }
            }
        }

        // polling (flush?) the ethernet interface
        let timestamp = millis_since(startup_time);
        let _next_poll_time = match iface.poll(&mut sockets, timestamp){
            Ok(t) => t,
            Err(err) => { 
                warn!("server_init(): poll error: {}", err);
                continue;
            }
        };  
    }
}

/// Function to calculate time since a give time in ms
fn millis_since(start_time: u64) -> u64 {
    let end_time: u64 = get_hpet().as_ref().unwrap().get_counter();
    let hpet_freq: u64 = get_hpet().as_ref().unwrap().counter_period_femtoseconds() as u64;
    // Convert to ms
    (end_time - start_time) * hpet_freq / 1_000_000_000_000
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
