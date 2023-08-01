//! To run this application in QEMU, ICMP must be enabled using the following
//! command:
//! ```sh
//! sudo sh -c "echo \"0 2147483647\" > /proc/sys/net/ipv4/ping_group_range"
//! ```

#![no_std]

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};
use app_io::println;
use core::{str::FromStr, time::Duration};
use getopts::{Matches, Options};
use net::{
    icmp::{Endpoint, PacketBuffer, PacketMetadata, Socket},
    phy::ChecksumCapabilities,
    wire::{Icmpv4Packet, Icmpv4Repr},
    IpAddress,
};
use time::Instant;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optopt("c", "count", "stop after <count> replies", "<count>");
    opts.optopt(
        "i",
        "wait",
        "wait <wait> seconds between sending each packet (default: 1)",
        "<wait>",
    );
    opts.optopt(
        "s",
        "packet size",
        "send <size> data bytes in each packet (max: 248, default: 56)",
        "<size>",
    );
    opts.optopt(
        "t",
        "timeout",
        "exit after <timeout> seconds regardless of how many packets have been received",
        "<timeout>",
    );

    let matches = match opts.parse(args) {
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

    let interface = net::get_default_interface().ok_or("no network interfaces available")?;

    let count = matches
        .opt_get_default("c", u16::MAX)
        .map_err(|_| "invalid count")?;
    let wait = Duration::from_secs(
        matches
            .opt_get_default("i", 1)
            .map_err(|_| "invalid wait")?,
    );
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
    let timeout = matches
        .opt_get("t")
        .map_err(|_| "invalid timeout")?
        .map(Duration::from_secs);

    let data = vec![0; packet_size];

    let rx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY], vec![0; 256]);
    let tx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY], vec![0; 256]);

    let socket = Socket::new(rx_buffer, tx_buffer);
    let socket = interface.clone().add_socket(socket);

    let mut num_sent = 0;
    let mut num_received = 0;

    let end = if let Some(timeout) = timeout {
        Instant::now() + timeout
    } else {
        Instant::MAX
    };
    let mut last_sent = Instant::ZERO;

    loop {
        let locked = socket.lock();

        let is_closed = !locked.is_open();
        let can_send = locked.can_send();
        let can_recv = locked.can_recv();

        drop(locked);

        if is_closed {
            socket
                .lock()
                .bind(Endpoint::Ident(0x22b))
                .map_err(|_| "failed to bind to endpoint")?;
        }

        let now = Instant::now();

        if can_send && num_sent < count && last_sent + wait <= now {
            last_sent = now;
            let repr = Icmpv4Repr::EchoRequest {
                ident: 0x22b,
                seq_no: num_sent,
                data: &data,
            };

            let mut locked = socket.lock();
            let payload = locked
                .send(repr.buffer_len(), remote)
                .map_err(|_| "failed to send packet")?;
            let mut packet = Icmpv4Packet::new_unchecked(payload);
            repr.emit(&mut packet, &ChecksumCapabilities::ignored());
            drop(locked);

            // Poll the socket to send the packet. Once we have a custom socket type this
            // won't be necessary.
            interface.poll();
            num_sent += 1;
        }

        if can_recv {
            let mut locked = socket.lock();
            let (payload, _) = locked.recv().map_err(|_| "failed to receive packet")?;
            let packet = Icmpv4Packet::new_checked(&payload)
                .map_err(|_| "incoming packet had incorrect length")?;
            let repr = Icmpv4Repr::parse(&packet, &ChecksumCapabilities::ignored())
                .map_err(|_| "failed to parse incoming packet")?;

            if let Icmpv4Repr::EchoReply { seq_no, .. } = repr {
                println!("{} bytes from {}: seq_no={}", payload.len(), remote, seq_no);
                drop(locked);
                num_received += 1;
            }
        }

        if num_received == count || end <= Instant::now() {
            let packet_loss = 100. - ((num_received as f64 / num_sent as f64) * 100.);
            println!("--- {remote} ping statistics ---");
            println!(
                "{num_sent} packets transmitted, {num_received} packets num_received, \
                 {packet_loss:.1}% packet loss",
            );
            return Ok(());
        }
    }
}

fn print_usage(opts: &Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &str = "Usage: ping DESTINATION
Pings an IPv4 address and displays network statistics";
