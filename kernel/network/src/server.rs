
use e1000::{E1000E_NIC};
use nw_server::{EthernetDevice};
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::*;
use alloc::borrow::{ToOwned};
use tsc::{tsc_ticks, TscTicks} ;
use smoltcp::Error;
use smoltcp::wire::{EthernetAddress, IpAddress};
use smoltcp::iface::{ArpCache, SliceArpCache, EthernetInterface};
use smoltcp::socket::{AsSocket, SocketSet,SocketHandle, SocketItem};
use smoltcp::socket::{UdpSocket, UdpSocketBuffer, UdpPacketBuffer};
use smoltcp::socket::{TcpSocket, TcpSocketBuffer};
use smoltcp::wire::{IpProtocol, IpEndpoint};
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use spin::{Once, Mutex};
use core::fmt;


pub static UDP_TEST_SERVER: Once<DFQueueProducer<String>> = Once::new();
//pub static UDP_TEST_SERVER: Once<DFQueueProducer<fmt::Arguments>> = Once::new();


pub fn test_server(_: Option<u64>) {

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

    let hardware_addr  = EthernetAddress([0x00, 0x0b, 0x82, 0x01, 0xfc, 0x42]); // 00:0b:82:01:fc:42 
    
    let protocol_addrs = [IpAddress::v4(192, 168, 69, 1)];
    let mut iface      = EthernetInterface::new(
        Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
        hardware_addr, protocol_addrs);  
    let mut timestamp_ms = 0;



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

     loop {

        {         
            //print!("UDP\n");

            
            let socket: &mut UdpSocket = sockets.get_mut(udp_handle).as_socket();
            if socket.endpoint().is_unspecified() {
                socket.bind(6969)
            }

            let start = tsc_ticks().to_ns().unwrap();

            


            let tuple = match socket.recv() {
                Ok((data, endpoint)) => {
                    debug!("received packet");
                    let mut data2 = data.to_owned();
                    let mut s = String::from_utf8(data2.to_owned()).unwrap();

                    //removing the newline

                    //s.pop();
                    let test_string = String::from("test\n");

                    if s.as_str() == test_string {
                        //client_endpoint = Some(endpoint.to_owned());
                        debug!("+++++++++++++++++++");
                    }
                    //client_endpoint = Some(endpoint.to_owned());
                    udpserver_producer.enqueue(s);

                    //debug!("Endpoint {:?}", endpoint);


                    Some((data2, endpoint))
                }
                Err(_) => {
                    None
                }
            };

            if let Some(endpoint) = client_endpoint{
                //let mut data = b"testing\n";
                use core::ops::Deref;
                let element = udpserver_consumer.peek();
                if !element.is_none() {
                    let element = element.unwrap();
                    let data = element.deref(); // event.deref() is the equivalent of   &*event     \

                    socket.send_slice(data.as_bytes(), endpoint).expect("sending failed");
                    element.mark_completed();
                    //client_endpoint = None;                     
                }


            }

            
            
        }



        let timestamp_ms = timestamp_ms + 1;
        let start = tsc_ticks().to_ns().unwrap();


        match iface.poll(&mut sockets, timestamp_ms) {
            Ok(()) | Err(Error::Exhausted) => (),
            Err(e) => debug!("poll error: {}", e)
        }
        

        
    }
    

}



use core::fmt::{Write};

/// An empty type for using the serial port with fmt::Write
pub struct StringArgument<'a> {
    pub buf: &'a mut [u8],
    pub length: usize,
}

impl<'a> StringArgument<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
       StringArgument {
            buf: buf,
            length: 0,
        }
    }

    pub fn write_fmt(&mut self, args: fmt::Arguments) -> ::core::fmt::Result {
        // /let mut serial = SERIAL_PORT.lock();
        self.write_fmt(args)
    }

    /// Write a str reference to the serial port.
    pub fn write_str(&mut self,s: &str) -> ::core::fmt::Result {
        //let mut serial = SERIAL_PORT.lock();
        self.write_str(s)
    }

}
impl<'a> fmt::Write for StringArgument <'a> {
    fn write_str(&mut self, s: &str) -> ::core::fmt::Result {
        self.buf.clone_from_slice(s.as_bytes());
        self.length = s.as_bytes().len();
        Ok(())
    }
}


 