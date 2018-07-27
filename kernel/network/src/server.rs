use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::borrow::{ToOwned};
use alloc::string::ToString;
use dfqueue::{DFQueue, DFQueueConsumer, DFQueueProducer};
use spin::{Once, Mutex};
use core::fmt;
use core::ops::Deref;
use acpi::get_hpet;
use smoltcp::Error;
use smoltcp::wire::{EthernetAddress, IpAddress};
use smoltcp::iface::{ArpCache, SliceArpCache, EthernetInterface};
use smoltcp::socket::{AsSocket, SocketSet,SocketHandle}; 
use smoltcp::socket::{UdpSocket, UdpSocketBuffer, UdpPacketBuffer};
use smoltcp::socket::{TcpSocket, TcpSocketBuffer};
use smoltcp::wire::{IpProtocol, IpEndpoint};
use smoltcp::phy::Device;
use e1000::{E1000_NIC,get_mac};
use e1000_to_smoltcp_interface::{EthernetDevice};
use config::*;


/// Static instance of the DFQueueProducer for the UDP_TEST_SERVER
/// When enquedued here, message will be sent to 
/// default mode: IP address 192.168.69.100, Port 5901
/// otherwise custom address
pub static UDP_TEST_SERVER: Once<DFQueueProducer<String>> = Once::new();

pub static CONFIG_IFACE: Once<DFQueueProducer<nw_iface_config>> = Once::new();

// Forwarding (host) IP address
pub static HOST_PORT: Once<u16> = Once::new();
pub static HOST_IP: Once<[u8;4]> = Once::new();

// Guest (machine) IP address
pub static GUEST_IP: Once<[u8;4]> = Once::new();

// Max buffer size for UDP socket buffers, can be tuned to get better performance
pub static UDP_SOCKET_BUFFER_SIZE: Once<usize> = Once::new();



pub struct network_ethernet_interface <'a, 'b, 'c, DeviceT: Device + 'a> {
    iface: EthernetInterface<'a, 'b, 'c, DeviceT>,
}


/// Initializing the test_server
/// This is invoked by the captain when the udp_server feature is set
/// default is used for IP address and port 
pub fn server_init(_: Option<u64>) {
    let startup_time = get_hpet().as_ref().unwrap().get_counter();;
    // Setting up udp buffer size, default size fis set to 8KB
    let mut skb_size:usize = 8*1024;
    if let Some(x_size) = UDP_SOCKET_BUFFER_SIZE.try(){
        skb_size = *x_size;
    } 

    // For UDP
    let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; skb_size])]);
    let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; skb_size])]);
    let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
    
    // For TCP - not implemented on the server
    // let tcp1_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    // let tcp1_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    // let tcp1_socket = TcpSocket::new(tcp1_rx_buffer, tcp1_tx_buffer);

    // let tcp2_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    // let tcp2_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    // let tcp2_socket = TcpSocket::new(tcp2_rx_buffer, tcp2_tx_buffer);

    let arp_cache = SliceArpCache::new(vec![Default::default(); 8]);

    let mut sockets:SocketSet = SocketSet::new(vec![]);

    let udp_handle  = sockets.add(udp_socket);
    // let tcp1_handle = sockets.add(tcp1_socket);
    // let tcp2_handle = sockets.add(tcp2_socket);

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

    let mut client_endpoint:Option<IpEndpoint> = None;
    let client_endpoint = Some(destination);

    //code to initialize the DFQ
    let udpserver_dfq: DFQueue<String> = DFQueue::new();
    let udpserver_consumer = udpserver_dfq.into_consumer();
    let udpserver_producer = udpserver_consumer.obtain_producer();
    UDP_TEST_SERVER.call_once(|| {
       udpserver_consumer.obtain_producer()
    });

    // Configuring the queue for the iface config
	let config_iface_dfq: DFQueue<nw_iface_config> = DFQueue::new();
    let config_iface_consumer = config_iface_dfq.into_consumer();
    let config_iface_producer = config_iface_consumer.obtain_producer();

    CONFIG_IFACE.call_once(|| {
       config_iface_consumer.obtain_producer()
    });
    
    // Main loop for the server
    loop {

        {   
			/// Configuring the mirror log server
			let element = config_iface_consumer.peek();
			if !element.is_none() {
				let element = element.unwrap();
				let data = element.deref(); // event.deref() is the equivalent of   &*event     
                // let cmd = match parse_mirror_log_to_nw_command(data.to_string()){
                //     Ok(cmd_type) => 
                //     {
                //         if cmd_type == SET_DESTINATION_IP {

                //         }
                //         else if cmd_type == SET_DESTINATION_PORT {

                //         }
                //         else {
                //             debug!("Command type not supported");
                //         }
                //     },
                //     Err(err) => debug!("{}",err.to_string()),
                // };
				element.mark_completed();
				//client_endpoint = None;                     
			}			

            /// UDP       
            let socket: &mut UdpSocket = sockets.get_mut(udp_handle).as_socket();
            if !socket.is_open() {
                socket.bind(6969).unwrap()
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
                let element = udpserver_consumer.peek();
                if !element.is_none() {
                    let element = element.unwrap();
                    let data = element.deref(); // event.deref() is the equivalent of   &*event     
                    let ret = match socket.send_slice(data.as_bytes(), endpoint){
                        Ok(_) => (),
                        Err(err) => debug!("UDP sending error {}",err.to_string()),
                    };
                    element.mark_completed();
                    //client_endpoint = None;                     
                }
            }                  
        }

        // polling the ethernet interface
        let timestamp = millis_since(startup_time);
        let poll_at = match iface.poll(&mut sockets, timestamp){
            Ok(_) => (),
            Err(err) => debug!("poll error {}",err.to_string()),
        };     
    }
}

/// Function to calculate time since a give time in ms
pub fn millis_since(start_time:u64)-> u64 {
    let end_time : u64 = get_hpet().as_ref().unwrap().get_counter();
    let hpet_freq : u64 = get_hpet().as_ref().unwrap().counter_period_femtoseconds() as u64;
    // Converting to ms
    (end_time-start_time)*hpet_freq/1000000000
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

// Initializing udp socket buffer size
pub fn set_udp_skb_size(skb_size:usize){
    UDP_SOCKET_BUFFER_SIZE.call_once(|| {
            skb_size
    });
}






