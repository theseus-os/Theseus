use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::borrow::{ToOwned};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use spin::{Once, Mutex};
use core::fmt;
use smoltcp::Error;
use smoltcp::wire::{EthernetAddress, IpAddress};
use smoltcp::iface::{ArpCache, SliceArpCache, EthernetInterface};
use smoltcp::socket::{AsSocket, SocketSet,SocketHandle, SocketItem}; 
use smoltcp::socket::{UdpSocket, UdpSocketBuffer, UdpPacketBuffer};
use smoltcp::socket::{TcpSocket, TcpSocketBuffer};
use smoltcp::wire::{IpProtocol, IpEndpoint};
use e1000::{E1000_NIC};
use e1000_to_smoltcp_interface::{EthernetDevice};



/// Static instance of the DFQueueProducer for the UDP_TEST_SERVER
/// When enquedued here, message will be sent to 
/// IP address 192.168.69.100
/// Port 5901
pub static UDP_TEST_SERVER: Once<DFQueueProducer<String>> = Once::new();


/// Initializing the test_server, should be called from the captain
pub fn server_init(_: Option<u64>) {

    // For UDP

    // max sizes for the buffer - change if needed
    let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 1024])]);
    let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 1024])]);
    let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
    
    // For TCP - not implemented on the server
    let tcp1_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    let tcp1_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    let tcp1_socket = TcpSocket::new(tcp1_rx_buffer, tcp1_tx_buffer);

    let tcp2_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    let tcp2_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    let tcp2_socket = TcpSocket::new(tcp2_rx_buffer, tcp2_tx_buffer);

    let arp_cache = SliceArpCache::new(vec![Default::default(); 8]);

    let mut sockets:SocketSet = SocketSet::new(vec![]);

    let udp_handle  = sockets.add(udp_socket);
    let tcp1_handle = sockets.add(tcp1_socket);
    let tcp2_handle = sockets.add(tcp2_socket);

    let mut tcp_6970_active = false;


    let device = EthernetDevice{
        tx_next: 0,
        rx_next: 0,
    };

    let hardware_addr  = EthernetAddress([0x00, 0x0b, 0x82, 0x01, 0xfc, 0x42]); // 00:0b:82:01:fc:42 
    
    let protocol_addrs = [IpAddress::v4(192, 168, 69, 1)];
    let mut iface      = EthernetInterface::new(
        Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
        hardware_addr, protocol_addrs);  
    let mut timestamp_ms = 0;


    // target address and port - forwarded udp packets will be sent to this
    let host_addrs = IpAddress::v4(192, 168, 69, 100);
    let destination = IpEndpoint{
        addr:host_addrs,
        port: 5901,
    };

    let mut client_endpoint:Option<IpEndpoint> = None;
    let client_endpoint = Some(destination);

    //code to initialize the DFQ
    let udpserver_dfq: DFQueue<String> = DFQueue::new();
    let udpserver_consumer = udpserver_dfq.into_consumer();
    let udpserver_producer = udpserver_consumer.obtain_producer();

    UDP_TEST_SERVER.call_once(|| {
       udpserver_consumer.obtain_producer()
    });

    // Main loop for the server
    loop {

        {         
            /// UDP       
            let socket: &mut UdpSocket = sockets.get_mut(udp_handle).as_socket();
            if socket.endpoint().is_unspecified() {
                socket.bind(6969)
            }

            // Receiving packets
            let tuple = match socket.recv() {
                Ok((data, endpoint)) => {
                    let mut data2 = data.to_owned();
                    let mut s = String::from_utf8(data2.to_owned()).unwrap();
                    //echoing the received udp msg
                    udpserver_producer.enqueue(s);                   
                    Some((data2, endpoint))
                }
                Err(_) => {
                    None
                }
            };

            // Sending packets
            if let Some(endpoint) = client_endpoint{
                use core::ops::Deref;
                let element = udpserver_consumer.peek();
                if !element.is_none() {
                    let element = element.unwrap();
                    //debug!("haako");
                    let data = element.deref(); // event.deref() is the equivalent of   &*event     
                    socket.send_slice(data.as_bytes(), endpoint).expect("sending failed");
                    element.mark_completed();
                    //client_endpoint = None;                     
                }
            }                  
        }

        // polling the ethernet interface
        let timestamp_ms = timestamp_ms + 1;
        match iface.poll(&mut sockets, timestamp_ms) {
            Ok(()) | Err(Error::Exhausted) => (),
            Err(e) => debug!("poll error: {}", e)
        }      
    }
}

// Setting up host machine if QEMU is used
// sudo ip tuntap add name tap0 mode tap user $USER
// sudo ip link set tap0 up
// sudo ip addr add 192.168.69.100/24 dev tap0
//
// Sending UDP packet to using socat
// socat stdio udp4-connect:192.168.66969 <<<"abcdefg"
//
// Sample test program that can be used from the connected machine to receive the udpo packets
// use std::net::UdpSocket;
// use std::str;
// 
// fn main() {  
// 	let socket = UdpSocket::bind("192.168.69.100:5901").expect("couldn't bind to address");
// 	let mut i = 0;
// 	while i < 20 {
// 		let mut buf = [0; 1000];
// 		let (number_of_bytes, src_addr) = socket.recv_from(&mut buf).expect("Didn't receive data");
//		let filled_buf = &mut buf[..number_of_bytes];
//
//		let mut s = str::from_utf8(filled_buf);
//		match s {
//        	Result::Ok(s1) => println!("{}",s1),
//           Result::Err(err) => (),
//        }
//
//		i = i +1;
// 	}
// }
// 
