#![no_std]

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};
use core::str::FromStr;
use net::{
    phy::ChecksumCapabilities,
    socket::icmp::{Endpoint, PacketBuffer, PacketMetadata, Socket},
    wire::{Icmpv4Packet, Icmpv4Repr},
    IpAddress, SocketSet,
};

pub fn main(_: Vec<String>) -> isize {
    let remote = IpAddress::from_str("142.250.80.78").unwrap();

    let iface = net::get_interfaces().lock().iter().next().cloned().unwrap();

    let rx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY], vec![0; 256]);
    let tx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY], vec![0; 256]);

    let socket = Socket::new(rx_buffer, tx_buffer);

    let mut sockets = SocketSet::new(vec![]);
    let handle = sockets.add(socket);

    let mut seq_no = 0;
    let mut received = 0;

    loop {
        log::info!("1");
        iface.lock().poll(&mut sockets);
        log::info!("2");

        let socket = sockets.get_mut::<Socket>(handle);
        log::info!("3");

        if !socket.is_open() {
            log::info!("4");
            socket.bind(Endpoint::Ident(0x22b)).unwrap();
            log::info!("5");
        }

        if socket.can_send()  && seq_no < 5 {
            log::info!("6");
            let repr = Icmpv4Repr::EchoRequest { ident: 0x22b, seq_no, data: &[] };

            let payload = socket.send(repr.buffer_len(), remote).unwrap();
            log::info!("7");
            let mut packet = Icmpv4Packet::new_unchecked(payload);
            log::info!("8");

            repr.emit(&mut packet, &ChecksumCapabilities::ignored());
            log::info!("9");
            seq_no += 1;
        }

        log::info!("10");

        if socket.can_recv() {
            log::info!("11");
            let (payload, _) = socket.recv().unwrap();
            log::info!("12");
            let packet = Icmpv4Packet::new_checked(&payload).unwrap();
            let repr = Icmpv4Repr::parse(&packet, &ChecksumCapabilities::ignored()).unwrap();

            log::info!("13");
            if let Icmpv4Repr::EchoReply { seq_no: _recvd_seq_no, .. } = repr {
                log::info!("14");
                return 0;
            }
            log::info!("15");
            received += 1;
        }
        
        if received == 5 {
            return 0;
        }
    }

    // -2
}
