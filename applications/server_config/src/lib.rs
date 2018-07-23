#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate print;

extern crate network;
extern crate getopts;
extern crate spawn;

use getopts::Options;
use alloc::{Vec, String};
use network::server::{server_init,set_host_ip_port,set_host_ip_address,set_guest_ip_address};

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    
    opts.optflag("h", "help", "print this help menu");
    
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{} \n", _f);
            return -1; 
        }
    };
    
    if matches.opt_present("h") {
        return print_usage(opts);
    }

    let mut guest_ip: [u8; 4] = [0;4];
    let mut host_ip: [u8; 4] = [0;4];
    let mut host_port:u16 = 0;
    // Parsing IP addresses and Port values
    let mut arg_count = 0;
    for arg in matches.free.iter() {
        println!("args for server_config {} \n", arg);
        arg_count = arg_count + 1;
        // parsing guest ip
        if arg_count == 1 {
            let split = arg.split(".");
            if split.clone().count()!= 4 {
                println!("Invalid Guest IP address");
                return -1;
            }

            let mut x_count = 0;
            for x in split{
                 match x.parse::<u8>(){
                     Ok(y) => {
                         guest_ip[x_count] = y;
                         x_count = x_count + 1;
                     }
                     _ => {
                        println!("Invalid Guest IP address");
                        return -1;
                     }

                 }
            }
        }
        // parsing host ip and port
        if arg_count == 2 {
            let split1 = arg.split("."); 
            if split1.clone().count()!= 2 {
                println!("Invalid HOST IP:PORT");
                return -1;
            }

            let mut t_count = 0;
            for split in split1 {
                if t_count == 0 {
                    let split = arg.split(".");
                    if split.clone().count()!= 4 {
                        println!("Invalid HOST IP address");
                        return -1;
                    }

                    let mut x_count = 0;
                    for x in split{
                        match x.parse::<u8>(){
                            Ok(y) => {
                                host_ip[x_count] = y;
                                x_count = x_count + 1;
                            }
                            _ => {
                                println!("Invalid Host IP address");
                                return -1;
                            }

                        }
                    }
                    t_count = t_count + 1;
                }

                if t_count == 1 {
                    match split.parse::<u16>(){
                        Ok(y) => {
                            host_port = y;
                        }
                        _ => {
                            println!("Invalid Host IP Port");
                            return -1;
                        }

                    }
                }
            }
        }
    }
    if arg_count!= 2 {
        println!("Invalid number of arguments");
        return -1;
    }


    set_host_ip_address(host_ip[0], host_ip[1], host_ip[2], host_ip[3]);
    set_guest_ip_address(guest_ip[0], guest_ip[1], guest_ip[2], guest_ip[3]);
    set_host_ip_port(host_port);
    spawn::spawn_kthread(server_init, None, String::from("starting up udp server"), None).unwrap();

    0
}

fn print_usage(opts: Options) -> isize {
    let brief = format!("Usage: server_config GUEST_IP HOST_IP:PORT");
    println!("{} \n", opts.usage(&brief));
    0
}

