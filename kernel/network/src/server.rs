#[macro_use]
// extern crate log;
// extern crate env_logger;
// extern crate getopts;
//extern crate smoltcp;

//mod utils;

/*use std::str;
use std::fmt::Write;
use std::time::Instant;
use std::os::unix::io::AsRawFd;*/
//extern crate collections;

use e1000::{E1000_NIC};
use nw_server::{EthernetDevice};
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::*;
use alloc::borrow::{ToOwned};



use smoltcp::Error;
use smoltcp::wire::{EthernetAddress, IpAddress};
use smoltcp::iface::{ArpCache, SliceArpCache, EthernetInterface};
use smoltcp::socket::{AsSocket, SocketSet,SocketHandle, SocketItem};
use smoltcp::socket::{UdpSocket, UdpSocketBuffer, UdpPacketBuffer};
use smoltcp::socket::{TcpSocket, TcpSocketBuffer};
use smoltcp::wire::{IpProtocol, IpEndpoint};
//use managed::ManagedSlice;
//use smoltcp::socket::set::Item;



//use smoltcp::phy::{TapInterface};

// #[derive(Debug)]
// pub struct Server<'a, 'b: 'a, 'c: 'a + 'b> {
//     sockets: SocketSet<'a, 'b, 'c>
// }

// impl <'a, 'b: 'a, 'c: 'a + 'b>  Server<'a, 'b, 'c>{
//     pub fn new() -> Server  <'a, 'b, 'c>{

//         let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 64])]);
//         let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 128])]);
//         let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
        
//         let tcp1_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
//         let tcp1_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
//         let tcp1_socket = TcpSocket::new(tcp1_rx_buffer, tcp1_tx_buffer);

//         let tcp2_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
//         let tcp2_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
//         let tcp2_socket = TcpSocket::new(tcp2_rx_buffer, tcp2_tx_buffer);

//         let arp_cache = SliceArpCache::new(vec![Default::default(); 8]);
//         let mut sockets = SocketSet::new(vec![]);


//         let udp_handle  = sockets.add(udp_socket);
//         let tcp1_handle = sockets.add(tcp1_socket);
//         let tcp2_handle = sockets.add(tcp2_socket);


//         let socket: &mut UdpSocket = sockets.get_mut(udp_handle).as_socket();
        
//         let mut tcp_6970_active = false;

//         let device = EInterface::new();


//         let hardware_addr  = EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
//         let protocol_addrs = [IpAddress::v4(192, 168, 69, 1)];
//         let mut iface      = EthernetInterface::new(
//             Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
//             hardware_addr, protocol_addrs);


//         Server {
//             sockets:   sockets
//         }
//     }
// }

pub fn test_server(_: Option<u64>) {
    debug!("++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++");
    debug!("test tap");
    debug!("++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++");
    
    let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 64])]);
    let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 128])]);
    let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
    
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

    // let device = EInterface::new();
    let device = EthernetDevice{
        tx_next: 0,
        rx_next: 0,
    };
    //let tmp: &mut [u8] = vec![0;5];



    //let hardware_addr  = EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    //let hardware_addr  = EthernetAddress([0x52, 0x55, 0x00, 0xd1, 0x55, 0x01]); //mac=52:55:00:d1:55:01
    let hardware_addr  = EthernetAddress([0x00, 0x0b, 0x82, 0x01, 0xfc, 0x42]); // 00:0b:82:01:fc:42 
    
    let protocol_addrs = [IpAddress::v4(192, 168, 69, 1)];
    let mut iface      = EthernetInterface::new(
        Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
        hardware_addr, protocol_addrs);  
    let mut timestamp_ms = 0;



    let host_addrs = [IpAddress::v4(192, 168, 69, 100)];
    
    // let host_mac  = EthernetAddress([0x46, 0x01, 0x63, 0x97, 0x5f, 0x73]); //46:01:63:97:5f:73
    // let host_ep = IpEndpoint::new(host_mac, 6969);

     loop {

        {         
            //print!("UDP\n");
            
            let socket: &mut UdpSocket = sockets.get_mut(udp_handle).as_socket();
            if socket.endpoint().is_unspecified() {
                socket.bind(6969)
            }


            let tuple = match socket.recv() {
                Ok((data, endpoint)) => {
                    debug!("udp:6969 recv data: {:?} from {}",
                           str::from_utf8(data.as_ref()).unwrap(), endpoint);
                    let mut data2 = data.to_owned();

                    Some((data2, endpoint))
                }
                Err(_) => {
                    None
                }
            };
            if let Some((data, endpoint)) = tuple {
                //let data2 = b"yo dawg\n";
                socket.send_slice(&data[..], endpoint).unwrap();
            }
        }

        //tcp:6969: respond "yo dawg"
        {
            let socket: &mut TcpSocket = sockets.get_mut(tcp1_handle).as_socket();
            if !socket.is_open() {
                socket.listen(6969).unwrap();
            }

            if socket.can_send() {
                //let data = b"yo dawg\n";
                let mut data = socket.recv(128).unwrap().to_owned();
                debug!("tcp:6969 send data: {:?}",
                       str::from_utf8(data.as_ref()).unwrap());
                //socket.send_slice(data).unwrap();
                socket.send_slice(&data[..]).unwrap();
                debug!("tcp:6969 close");
                socket.close();
            }
        }

                // tcp:6970: echo with reverse
        {
            let socket: &mut TcpSocket = sockets.get_mut(tcp2_handle).as_socket();
            if !socket.is_open() {
                socket.listen(6970).unwrap()
            }

            if socket.is_active() && !tcp_6970_active {
                debug!("tcp:6970 connected");
            } else if !socket.is_active() && tcp_6970_active {
                debug!("tcp:6970 disconnected");
            }
            tcp_6970_active = socket.is_active();

            if socket.may_recv() {
                let data = {
                    let mut data = socket.recv(128).unwrap().to_owned();
                    if data.len() > 0 {
                        debug!("tcp:6970 recv data: {:?}",
                               str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                        // data = data.split(|&b| b == b'\n').collect::<Vec<_>>().concat();
                        // data.reverse();
                        // data.extend(b"\n");
                    }
                    data
                };
                if socket.can_send() && data.len() > 0 {
                    debug!("tcp:6970 send data: {:?}",
                           str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                    socket.send_slice(&data[..]).unwrap();
                }
            } else if socket.may_send() {
                debug!("tcp:6970 close");
                socket.close();
            }
        }

        let timestamp_ms = timestamp_ms + 1;
        match iface.poll(&mut sockets, timestamp_ms) {
            Ok(()) | Err(Error::Exhausted) => (),
            Err(e) => debug!("poll error: {}", e)
        }
        //debug!("++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++");

        
    }
    

}


