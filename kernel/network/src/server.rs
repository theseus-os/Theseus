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
use e1000;
use e1000_to_smoltcp_interface::{EthernetDevice};
use logger::LogColor;


static MSG_QUEUE: Once<DFQueueProducer<String>> = Once::new();


pub fn server_init(_: Option<u64>) {

    let e1000_nic_ref = match e1000::E1000_NIC.try() {
        Some(nic) => nic,
        None => {
            error!("server_init(): E1000 NIC has not been initialized yet!");
            return;
        }
    };

    let startup_time = get_hpet().as_ref().unwrap().get_counter();;
    let mut skb_size = 8*1024; // 8KiB 

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

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now

    let udp_handle = sockets.add(udp_socket);
    // let tcp1_handle = sockets.add(tcp1_socket);
    // let tcp2_handle = sockets.add(tcp2_socket);

    let device = EthernetDevice{
        tx_next: 0,
        rx_next: 0,
    };


    // getting the mac address from the ethernet device
    let hardware_mac_addr = EthernetAddress(e1000_nic_ref.lock().mac_address());

    // setup destination address and port
    let dest_addr = IpAddress::v4(192, 168, 69, 100); // the default gateway (router?)
    let dest_port = 5901;

    // setup host ip address (this machine)
    let local_addr = [IpAddress::v4(192, 168, 69, 1)];
    let local_port = 6969;

    // Initializing the Ethernet interface
    let mut iface = EthernetInterface::new(
        Box::new(device), Box::new(arp_cache) as Box<ArpCache>,
        hardware_mac_addr, local_addr);  
    let mut timestamp_ms = 0;


    let dest_endpoint = IpEndpoint {
        addr: dest_addr,
        port: dest_port,
    };

    //code to initialize the DFQ
    let udpserver_dfq: DFQueue<String> = DFQueue::new();
    let udpserver_consumer = udpserver_dfq.into_consumer();
    let udpserver_producer = udpserver_consumer.obtain_producer();
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
            let tuple = match udp_socket.recv() {
                Ok((data, endpoint)) => {
                    let mut data2 = data.to_owned();
                    let mut s = String::from_utf8(data2.to_owned()).unwrap();
                    //echoing the received udp msg
                    udpserver_producer.enqueue(s);
                    let tuple = (data2, endpoint);
                    debug!("Received tuple: {:?}", tuple);
                    Some(tuple)
                }
                _ => None,
            };

            // Sending packets
            match udpserver_consumer.peek() {
                Some(element) => {
                    debug!("server_init(): about to send UDP packet {:?}", &*element);
                    if let Err(_e) = udp_socket.send_slice(element.as_bytes(), dest_endpoint) {
                        error!("server_init(): UDP sending error {}", _e);
                        // break;
                    }
                    element.mark_completed();
                }
                _ => { }
            }
        }

        // polling (flush?) the ethernet interface
        let timestamp = millis_since(startup_time);
        let _next_poll_time = match iface.poll(&mut sockets, timestamp){
            Ok(t) => t,
            Err(err) => { 
                error!("server_init(): poll error: {}", err);
                break;
            }
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
pub fn send_log_msg_udp(_color: &LogColor, prefix: &'static str, msg: fmt::Arguments){
    if let Some(producer) = MSG_QUEUE.try(){
        let s = format!("{}{}", prefix, msg);
        producer.enqueue(s.to_string());  
    }
}

/// Function to send a message using UDP to configured destination
pub fn send_msg_udp<T: ToString>(msg: T){
    if let Some(producer) = MSG_QUEUE.try(){
        producer.enqueue(msg.to_string());  
    }
}
