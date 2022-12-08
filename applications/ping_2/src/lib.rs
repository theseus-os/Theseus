#![no_std]

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};
use app_io::{print, println};
use core::str::FromStr;
use getopts::{Matches, Options};
use net::{
    phy::ChecksumCapabilities,
    socket::icmp::{Endpoint, PacketBuffer, PacketMetadata, Socket},
    wire::{Icmpv4Packet, Icmpv4Repr},
    IpAddress, SocketSet,
};

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt(
        "c",
        "count",
        "stop after <count> replies (default: 4)",
        "<count>",
    );
    opts.optopt(
        "s",
        "packet size",
        "send <size> data bytes in each packet (max: 248, default: 56)",
        "<size>",
    );

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(&opts);
            return -1;
        }
    };

    if matches.opt_present("h") {
        print_usage(&opts);
        0
    } else {
        match _main(matches) {
            Ok(_) => 0,
            Err(e) => {
                println!("{}", e);
                -1
            }
        }
    }
}

fn _main(matches: Matches) -> Result<(), &'static str> {
    let remote = IpAddress::from_str(matches.free.get(0).ok_or("no arguments_provided")?)
        .map_err(|_| "invalid argument")?;

    let iface = net::get_default_interface().ok_or("no network interfaces available")?;

    let count = matches
        .opt_get_default("c", 4)
        .map_err(|_| "invalid count")?;
    let packet_size = {
        let packet_size = matches
            .opt_get_default("s", 56)
            .map_err(|_| "invalid packet size")?;
        if packet_size > 248 {
            return Err("packet size too large");
        } else {
            packet_size
        }
    };

    let data = vec![0; packet_size];

    let rx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY], vec![0; 256]);
    let tx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY], vec![0; 256]);

    let socket = Socket::new(rx_buffer, tx_buffer);

    let mut sockets = SocketSet::new(vec![]);
    let handle = sockets.add(socket);

    let mut sent = 0;
    let mut received = 0;

    loop {
        iface.poll(&mut sockets).map_err(|_| "failed to poll socket")?;

        let socket = sockets.get_mut::<Socket>(handle);

        if !socket.is_open() {
            socket
                .bind(Endpoint::Ident(0x22b))
                .map_err(|_| "failed to bind to endpoint")?;
        }

        if socket.can_send() && sent == received && sent < count {
            let repr = Icmpv4Repr::EchoRequest {
                ident: 0x22b,
                seq_no: sent,
                data: &data,
            };

            let payload = socket
                .send(repr.buffer_len(), remote)
                .map_err(|_| "failed to send packet")?;
            let mut packet = Icmpv4Packet::new_unchecked(payload);

            repr.emit(&mut packet, &ChecksumCapabilities::ignored());
            sent += 1;
        }

        if socket.can_recv() {
            let (payload, _) = socket.recv().map_err(|_| "failed to receive packet")?;
            let packet = Icmpv4Packet::new_checked(&payload)
                .map_err(|_| "incoming packet had incorrect length")?;
            let repr = Icmpv4Repr::parse(&packet, &ChecksumCapabilities::ignored())
                .map_err(|_| "failed to parse incoming packet")?;

            if let Icmpv4Repr::EchoReply { seq_no, .. } = repr {
                println!("{} bytes from {}: seq_no={}", payload.len(), remote, seq_no);
                received += 1;
            }
        }

        if received == count {
            return Ok(());
        }
    }
}

fn print_usage(opts: &Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &str = "Usage: ping DESTINATION
Pings an IPv4 address and displays network statistics";
