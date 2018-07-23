use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::borrow::{ToOwned};
use alloc::string::ToString;
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
use e1000::{E1000_NIC,get_mac};
use e1000_to_smoltcp_interface::{EthernetDevice};



/// Static instance of the DFQueueProducer for the UDP_TEST_SERVER
/// When enquedued here, message will be sent to 
/// default mode: IP address 192.168.69.100, Port 5901
/// otherwise custom address
pub static UDP_TEST_SERVER: Once<DFQueueProducer<String>> = Once::new();

// Forwarding (host) IP address
pub static HOST_PORT: Once<u16> = Once::new();
pub static HOST_IP: Once<[u8;4]> = Once::new();

// Guest (machine) IP address
pub static GUEST_IP: Once<[u8;4]> = Once::new();


/// Initializing the test_server
/// This is invoked by the captain when the udp_server feature is set
/// default is used for IP address and port 
pub fn server_init(_: Option<u64>) {

    // For UDP
    debug!("starting udp");
    // max sizes for the buffer 
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

    // getting the mac address from the ethernet device
    let mac             = get_mac();
    let hardware_addr   = EthernetAddress(mac);

    // initializing forwarding address (host)
    // default is set to 192.168.69.1:5901
    let mut forwarding_port:u16 = 5901;
    if let Some(port) = HOST_PORT.try(){
        forwarding_port = *port;
    } 
    let mut host_addrs = IpAddress::v4(192, 168, 69, 100);
    if let Some(addr) = HOST_IP.try(){
        host_addrs = IpAddress::v4(addr[0], addr[1], addr[2], addr[3]);
    }  

    // Initializing GUEST IP address
    // default set to 192.168.69.100 and any port
    let mut protocol_addrs = [IpAddress::v4(192, 168, 69, 1)];
    if let Some(addr) = GUEST_IP.try(){
        protocol_addrs = [IpAddress::v4(addr[0], addr[1], addr[2], addr[3])];
    }    

    // Initializing the Ethernet interface
    let mut iface      = EthernetInterface::new(
        Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
        hardware_addr, protocol_addrs);  
    let mut timestamp_ms = 0;


    let destination = IpEndpoint{
        addr: host_addrs,
        port: forwarding_port,
    };


    // target address and port - forwarded udp packets will be sent to this
    //commented from old
    /*let host_addrs = IpAddress::v4(192, 168, 69, 100);
    let destination = IpEndpoint{
        addr:host_addrs,
        port: 5901,
    };*/

    let mut client_endpoint:Option<IpEndpoint> = None;
    let client_endpoint = Some(destination);

    //code to initialize the DFQ
    let udpserver_dfq: DFQueue<String> = DFQueue::new();
    let udpserver_consumer = udpserver_dfq.into_consumer();
    let udpserver_producer = udpserver_consumer.obtain_producer();

    UDP_TEST_SERVER.call_once(|| {
       udpserver_consumer.obtain_producer()
    });
    debug!("before loop");
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

/// Function to send debug messages using UDP to configured destination
/// Used to mirror debug messages 
pub fn send_debug_msg_udp(msg: fmt::Arguments){
    if let Some(producer) = UDP_TEST_SERVER.try(){
        let s = format!("{}", msg);
        producer.enqueue(s.to_string());  
    }
}

/// Function to send a message using UDP to configured destination
pub fn send_msg_udp<T:ToString>(msg: T){
    if let Some(producer) = UDP_TEST_SERVER.try(){
        producer.enqueue(msg.to_string());  
    }
}


// Intializing (host) IP address and port
pub fn set_host_ip_port (port:u16){
    HOST_PORT.call_once(|| {
            port
    });
}
pub fn set_host_ip_address (ip0:u8,ip1:u8,ip2:u8,ip3:u8){
    HOST_IP.call_once(|| {
           [ip0,ip1,ip2,ip3]
    });
}

// Intializing guest IP address
pub fn set_guest_ip_address (ip0:u8,ip1:u8,ip2:u8,ip3:u8){
    GUEST_IP.call_once(|| {
           [ip0,ip1,ip2,ip3]
    });
}

