//! This application pings a specific IPv4 address and gets ping statistics.
//! Important: QEMU does not support the ICMP protocol by default so it's important to 
//! run this command: sudo sh -c "echo \"0 2147483647\" > /proc/sys/net/ipv4/ping_group_range"
//! in the environment prior to running this application

#![no_std]
#![feature(slice_concat_ext)]

#[macro_use] extern crate log;
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate smoltcp;
extern crate network_manager;
extern crate byteorder;
extern crate hpet;
extern crate http_client;
extern crate hashbrown;
extern crate ota_update_client;
extern crate getopts;

use getopts::{Matches, Options};
use core::str::FromStr;
use hashbrown::HashMap;
use alloc::vec::Vec;        
use alloc::string::String;
use hpet::get_hpet;
use smoltcp::{
    socket::{SocketSet, IcmpSocket, IcmpSocketBuffer, IcmpPacketMetadata, IcmpEndpoint},
    wire::{IpAddress, Icmpv4Repr, Icmpv4Packet},
    phy::{ChecksumCapabilities},
};
use network_manager::{NetworkInterfaceRef, NETWORK_INTERFACES};
use byteorder::{ByteOrder, NetworkEndian};
use http_client::{millis_since, poll_iface};


macro_rules! hpet_ticks {
    () => {
        get_hpet().as_ref().ok_or("coudln't get HPET timer").unwrap().get_counter()
    };
}

macro_rules! send_icmp_ping {
    ( $repr_type:ident, $packet_type:ident, $ident:expr, $seq_no:expr,
    $echo_payload:expr, $socket:expr, $remote_addr:expr ) => {{
        let icmp_repr = $repr_type::EchoRequest {
            ident: $ident,
            seq_no: $seq_no,
            data: &$echo_payload,
        };

        let icmp_payload = $socket
            .send(icmp_repr.buffer_len(), $remote_addr)
            .unwrap();

        let icmp_packet = $packet_type::new_unchecked(icmp_payload);
        (icmp_repr, icmp_packet)
    }}
}


macro_rules! get_icmp_pong {
    ( $repr_type:ident, $repr:expr, $payload:expr, $waiting_queue:expr, $remote_addr:expr,
    $timestamp:expr, $received:expr, $total_time:expr ) => {{
        if let $repr_type::EchoReply { seq_no, data, .. }     = $repr {
            if let Some(_) = $waiting_queue.get(&seq_no) {
                let packet_timestamp_ms = NetworkEndian::read_i64(data);
                println!("{} bytes from {}: icmp_seq={}, time={}ms",
                        data.len(), $remote_addr, seq_no,
                        $timestamp - packet_timestamp_ms);
                $waiting_queue.remove(&seq_no);
                $received += 1;
                $total_time += $timestamp - packet_timestamp_ms;
            }
        }
    }}
}




#[no_mangle]
fn main(args: Vec<String>) -> isize {

    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt("c", "count", "Amount of echo request packets to send (default: 4)", "N");
    opts.optopt("i", "interval", "interval between packets being sent in miliseconds (default: 5000)", "N");
    opts.optopt("t", "timeout", "maximum time between echo request and echo reply in milliseconds(default: 1500)", "N");
    


    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(&opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        return print_usage(&opts);
        return 0;
    }

    let mut ping_address = IpAddress::Unspecified;

    if matches.free.len() != 0 {
        match IpAddress::from_str(&matches.free[0]) {
            Ok(address) => {
                ping_address = address;
                
            }
            _ => { 
                println!("Invalid argument {}, not a valid adress", matches.free[0]); 
                return -1;
            },
        };  
    
    }

    else {
        println!("no arguments provided");
        return 0;
    }
    
    let result = rmain(&matches, opts, ping_address);
    
    match result {
        Ok(_) => { 0 }
        Err(e) => {
            println!("Ping initialization failed: {}.", e);
            -1
        }
    }
}

pub fn rmain(matches: &Matches, opts: Options, address: IpAddress) -> Result<(), &'static str> {

    
    let mut count = 4;
    let mut interval = 500;
    let mut timeout = 15000;
    let did_work = true;


    if let Some(i) = matches.opt_default("c", "4") {
        count = i.parse::<usize>().map_err(|_e| "couldn't parse number of num_tasks")?;
    }
    if let Some(i) = matches.opt_default("i", "500") {
        interval = i.parse::<i64>().map_err(|_e| "couldn't parse interval")?;
    }
    if let Some(i) = matches.opt_default("t", "1500") {
        timeout = i.parse::<i64>().map_err(|_e| "couldn't parse interval")?;
    }
    

    if did_work {
        ping(address, count, interval, timeout);
        Ok(())
    }
    else {
        Ok(())
    }
}

//used to gain access to the ethernet interface
fn get_default_iface() -> Result<NetworkInterfaceRef, String> {
    NETWORK_INTERFACES.lock()
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| format!("no network interfaces available"))
}

fn ping(address: IpAddress, count: usize, interval: i64, timeout: i64) {

    let startup_time = hpet_ticks!() as u64;
    let remote_addr = address;
    
    //initialize the ICMP sockett using the smoltcp function using a transmit packet buffer 
    //and receiving packet buffer
    //
    //The payload storage contains the application data and the transport header, and metadata contains the ICMP type
    //and ICMP code, which are used to classify between echo requests and echo replies
    let icmp_rx_buffer = IcmpSocketBuffer::new(vec![IcmpPacketMetadata::EMPTY], vec![0; 256]);
    let icmp_tx_buffer = IcmpSocketBuffer::new(vec![IcmpPacketMetadata::EMPTY], vec![0; 256]);
    let icmp_socket = IcmpSocket::new(icmp_rx_buffer, icmp_tx_buffer);
    
    //get the default ethernet interface to ping with
    let iface = get_default_iface().unwrap(); 
    let mut sockets = SocketSet::new(vec![]);
    let icmp_handle = sockets.add(icmp_socket);
    let mut send_at = millis_since(startup_time as u64).unwrap() as i64; 
    let mut seq_no = 0;
    let mut received = 0;
    let mut total_time = 0;
    let mut echo_payload = [0xffu8; 40];

    //designate no checksum capabilities 
    let checksum_caps = ChecksumCapabilities::ignored();
    
    //initiate a hashmap
    let mut waiting_queue = HashMap::new();  
    
    //portless icmp messages such as echo request require a 16-bit identifier to bind to
    //so that only icmp message with this identifer can pass through the icmp socket
    let ident = 0x22b; 

    //makes sure that the icmp handle can communicate with the given ethernet interface
    loop {
        
        match poll_iface(&iface, &mut sockets, startup_time) {
            Ok(_) => {},
            Err(e) => {
                debug!("poll error: {}", e);
            }
        }
        {
            let timestamp = millis_since(startup_time as u64).unwrap() as i64;
            let mut socket = sockets.get::<IcmpSocket>(icmp_handle); 
            
            //checks if the icmp socket is open, and only bind the identifier icmp to it if 
            //it is closed
            if !socket.is_open() {
                println!("the socket is binded to");
                socket.bind(IcmpEndpoint::Ident(ident)).unwrap();
                send_at = timestamp;
            }
            
            //checks if the icmp sockett can send an echo request
            if socket.can_send() && seq_no < count as u16 && send_at <= timestamp 
                {
                println!("\n -- icmp_seq={} -- \n", seq_no);
                NetworkEndian::write_i64(&mut echo_payload, timestamp);
                let (icmp_repr, mut icmp_packet) = send_icmp_ping!(
                                Icmpv4Repr, Icmpv4Packet, ident, seq_no,
                                echo_payload, socket, remote_addr);
                
                icmp_repr.emit(&mut icmp_packet, &checksum_caps); //turns or "emits" the raw network stack into an icmpv4 packet,
                println!("buffer length: {}", icmp_repr.buffer_len());
                println!("checking checksum of packet, should be 0: {:?}", icmp_packet.checksum());
                println!("checking echo_ident of packet, should be a value: {:?}", icmp_packet.echo_ident());
                println!("checking msg_type of packet, should be an echo_request: {:?}", icmp_packet.msg_type());
            
            //insert the sequence number into the waiting que along with the timestamp after an echo
            //request has been sent
            waiting_queue.insert(seq_no, timestamp);
            seq_no += 1;
            send_at += interval;
            }

            //once the socket can successfully recieve the echo reply, unload the payload and unwrap it
            //then return the current time as well as wether the ping has been recieved         
            if socket.can_recv() {
                let (payload, _) = socket.recv().unwrap();
                let icmp_packet = Icmpv4Packet::new_checked(&payload).unwrap();
                let icmp_repr = 
                    Icmpv4Repr::parse(&icmp_packet, &checksum_caps).unwrap(); //turns or "parses" the ICMPv4 packet into a raw level representation
                    get_icmp_pong!(Icmpv4Repr, icmp_repr, payload,
                                waiting_queue, remote_addr, timestamp, received, total_time);
                println!("buffer length: {}", icmp_repr.buffer_len());
                println!("checking checksum of packet, should be above 0: {:?}", icmp_packet.checksum());
                println!("checking echo_ident of packet, should be a value: {:?}", icmp_packet.echo_ident());
                println!("checking msg_type of packet, should be an echo_reply: {:?}", icmp_packet.msg_type());        
                    }
            
            //uses this retain function to decide whether the sequence you're currently looking at is timed out 
            waiting_queue.retain(|seq, from| {
                if timestamp - *from <  timeout {
                    true
                } else {
                    println!("From {} icmp_seq={} timeout", remote_addr, seq);
                    false
                }
            });

            if seq_no == count as u16 && waiting_queue.is_empty()  {
                break
            }
        }
    

    }
    let mut avg_ping = 0;
    
    if received == 0 {
        
    }
    else {
        avg_ping = total_time/(received as i64);
    }
    
    println!("--- {} ping statistics ---", remote_addr);
    println!("{} packets transmitted, {} received, {:.0}% packet loss, {}ms average latency",
            seq_no, received, 100.0 * (seq_no - (received)) as f64 / seq_no as f64, avg_ping);
    


}

fn print_usage(opts: &Options) -> isize {
    let mut brief = format!("Usage: ping \n \n");

    brief.push_str("pings an IPv4 address and returns ping statistics");

    println!("{} \n", opts.usage(&brief));

    0
}
