

use e1000::{E1000E_NIC};
use nw_server::{EthernetDevice, TxBuffer};
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
use smoltcp::phy::Device;
use irq_safety::MutexIrqSafe;
use core::str::FromStr;
/* 
lazy_static! {
        pub static ref SERVER: MutexIrqSafe<Server<'static, EthernetDevice>> = MutexIrqSafe::new(Server{
              sockets  : SocketSet::new(vec![]),
              //udp_handle: None,
              iface : None,
              //tcp_6970_active: false        
        });

        // pub static ref SERVER: Server = Server<T>{
        //     iface : None,
        //  };
} */

lazy_static! {
    pub static ref SERVER:server_send = server_send{is_ready:false, string:String::from_str("").unwrap()};
}
pub struct server_send{
    is_ready:bool,
    string:String, 
    
}

impl server_send{

    pub fn reset(&mut self){
        self.is_ready = false;
        self.string = String::from_str("").unwrap();
    }
    pub fn set(&mut self, msg:String){
        self.is_ready = true;
        self.string = msg;
    }
}



pub struct Server_send{
    is_ready:bool,

    
}

/*
pub fn server_init(){
    let mut server_nw = SERVER.lock();
    server_nw.init();
}

pub struct Server<'a, T: Device + 'static> {
    sockets:SocketSet <'static, 'static, 'static>,
    udp_handle:Option<SocketHandle>,
    //iface : EthernetInterface<'static, 'static, 'static, T>,
    iface : EthernetInterface<'a, 'a, 'a, T>,

    //tcp_6970_active:bool, 
}

impl<'a, T: Device + 'static> Server<'a, T> {
        pub fn init(&mut self){
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


            self.udp_handle  = self.sockets.add(udp_socket);
            // //let tcp1_handle = sockets.add(tcp1_socket);
            // //let tcp2_handle = sockets.add(tcp2_socket);




            self.tcp_6970_active = false;

            let device = EthernetDevice{
                tx_next: 0,
                rx_next: 0,
            };

            let hardware_addr  = EthernetAddress([0x00, 0x0b, 0x82, 0x01, 0xfc, 0x42]); // 00:0b:82:01:fc:42 
            
            let protocol_addrs = [IpAddress::v4(192, 168, 69, 1)];
            let mut iface      = EthernetInterface::new(
                Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
                hardware_addr, protocol_addrs);  
            // let mut timestamp_ms = 0;


        }

        pub fn respond_interrupt(&mut self){

            {         
            //print!("UDP\n");
            
                let socket: &mut UdpSocket = self.sockets.get_mut(self.udp_handle).as_socket();
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

            
            
        }
}
*/

pub fn test_server2(_: Option<u64>) {

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



    let host_addrs = [IpAddress::v4(192, 168, 69, 100)];
    


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


