//! Functions to communicate with a network server that provides over-the-air live update functionality.
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
extern crate memory;
extern crate owning_ref;
extern crate spawn;
extern crate task;


use core::str::{self, FromStr};
use core::fmt::Write;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use dfqueue::{DFQueue, DFQueueProducer, DFQueueConsumer};
use spin::Once;
use acpi::get_hpet;
use smoltcp::wire::{EthernetAddress, IpAddress};
use smoltcp::iface::{ArpCache, SliceArpCache, EthernetInterface};
use smoltcp::socket::{AsSocket, SocketSet}; 
use smoltcp::socket::{TcpSocket, TcpSocketBuffer};
use e1000_smoltcp_device::E1000Device;
use network_interface_card::NetworkInterfaceCard;
use irq_safety::MutexIrqSafe;


/// The IP address of the update server.
/// This is currently the static IP of `kevin.recg.rice.edu`.
const DEFAULT_DESTINATION_IP_ADDR: [u8; 4] = [168, 7, 138, 84];

/// The TCP port on the update server that listens for update requests 
const DEFAULT_DESTINATION_PORT: u16 = 8098;


const DEFAULT_LOCAL_GATEWAY_IP: [u8; 4] = [192, 168, 1, 1];



static MSG_QUEUE: Once<DFQueueProducer<String>> = Once::new();


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

    // let dest_addr = IpAddress::v4(
    //     DEFAULT_DESTINATION_IP_ADDR[0],
    //     DEFAULT_DESTINATION_IP_ADDR[1],
    //     DEFAULT_DESTINATION_IP_ADDR[2],
    //     DEFAULT_DESTINATION_IP_ADDR[3],
    // );
    let dest_addr = IpAddress::v4(
        DEFAULT_LOCAL_GATEWAY_IP[0],
        DEFAULT_LOCAL_GATEWAY_IP[1],
        DEFAULT_LOCAL_GATEWAY_IP[2],
        DEFAULT_LOCAL_GATEWAY_IP[3],
    );
    let dest_port = DEFAULT_DESTINATION_PORT;

    // setup local ip address (this machine)
    let local_addr = IpAddress::v4(192, 168, 1, 44);
    let local_port = 6969;

    
    let arp_cache = SliceArpCache::new(vec![Default::default(); 8]);

    let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
    let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
    let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

    let mut sockets = SocketSet::new(Vec::with_capacity(1)); // just 1 socket right now
    let tcp_handle = sockets.add(tcp_socket);

    // Setup an interface/device that connects smoltcp to our e1000 driver
    let device = E1000Device::new(nic);
    let hardware_mac_addr = EthernetAddress(nic.lock().mac_address());
    let mut iface = EthernetInterface::new(
        Box::new(device), 
        Box::new(arp_cache) as Box<ArpCache>,
        hardware_mac_addr, 
        [local_addr]
    );  

    {
        let socket: &mut TcpSocket = sockets.get_mut(tcp_handle).as_socket();
        socket.connect((dest_addr, dest_port), (local_addr, local_port)).unwrap();
    }


    let mut tcp_active = false;
    let mut send_items = 0;
    let mut received_items = 0;

    loop {
        {
            let socket: &mut TcpSocket = sockets.get_mut(tcp_handle).as_socket();
            if socket.is_active() && !tcp_active {
                debug!("ota_update_client(): socket connected");
            } else if !socket.is_active() && tcp_active {
                error!("ota_update_client(): socket disconnected");
                return Err("socket was disconnected");
            }
            tcp_active = socket.is_active();

            warn!("SOCKET MAY_SEND: {:?},   CAN_SEND: {:?}", socket.may_send(), socket.can_send());
            if socket.may_send() {
                if socket.can_send() && send_items == 0 {
                    debug!("ota_update_client(): tcp:6969 send greeting");
                    //let s: String = number_of_items.to_string();
                    //write!(socket, "{}",s).unwrap();
                    write!(socket, "4").unwrap();
                    //socket.close();
                    warn!("ota_update_client(): Send 0");
                }
                if send_items == 1 {
                    write!(socket, "a#terminal_print.o").unwrap();
                    warn!("ota_update_client(): Send 1");
                }
                if send_items == 2 {
                    write!(socket, "a#test_panic.o").unwrap();
                    warn!("ota_update_client(): Send 2");
                }
                if send_items == 3 {
                    write!(socket, "k#acpi-b3db4f72ccdc307b.o").unwrap();
                    warn!("ota_update_client(): Send 3");
                }
                if send_items == 4 {
                    write!(socket, "k#ap_start-5e82a1be3db78c92.o").unwrap();
                    warn!("ota_update_client(): Send 4");
                }
                send_items = send_items + 1;
            }

            if socket.may_recv() {
                let data = {
                    let mut data = socket.recv(64).unwrap().to_vec();
                    if data.len() > 0 {
                        debug!("ota_update_client(): recv data: {:?}",
                               str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                        // let mut rec = data.len();
                        // data = data.split(|&b| b == b'\n').collect::<Vec<_>>().concat();
                        //data.reverse();
                        //data.extend(b"\n");
                        warn!("ota_update_client(): recv data: {:?}",
                               str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                        received_items = received_items + 1;
                        
                        //while rec < 64 {
                        //    let mut data2 = socket.recv(64).unwrap().to_owned();
                        //    rec = rec + data2.len();
                        //}
                    }
                    data
                };

                // if socket.can_send() && data.len() > 0{
                //     debug!("ota_update_client(): tcp:6969 send greeting");
                //     write!(socket, "12").unwrap();
                //     debug!("ota_update_client(): tcp:6969 close");
                //     //socket.close();
                //     initial_val_send = true; 
                // }
                
                // if socket.can_send() && data.len() > 0 {
                //     debug!("ota_update_client(): send data: {:?}",
                //            str::from_utf8(data.as_ref()).unwrap_or("(invalid utf8)"));
                //     socket.send_slice(&data[..]).unwrap();
                // }
            }
        }

        let timestamp = millis_since(startup_time);
        let _next_poll_time = match iface.poll(&mut sockets, timestamp) {
            Ok(t) => t,
            Err(err) => { 
                warn!("network_test::init(): poll error: {}", err);
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
