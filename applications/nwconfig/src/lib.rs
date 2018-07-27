#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate print;

extern crate network;
extern crate getopts;
extern crate spawn;

use getopts::Options;
use alloc::{Vec, String};
use alloc::string::ToString;
use network::server::{server_init,set_host_ip_port,set_host_ip_address,set_guest_ip_address};
use network::server::{add_nw_config_queue};
use network::config::*;

// constants
//static NO_ARG: &'static str      = "No arguments to the command \"ifconfig\"";
//static INVALID_ARG: &'static str = "Invalid arguments to the command \"ifconfig\"";




#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    // Config parameters
    let mut config_interface    :u8 = 0;
 

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

    let mut arg_vec = Vec::new();
    for arg in matches.free.iter() {
        arg_vec.push(arg);
    }

    // If no arguments are present, exit the application
    if arg_vec.len() == 0 {
        println!("{}", "No arguments to the command \"ifconfig\"");
        return -1
    }
    // At this moment we expect 3 arguments 
    if arg_vec.len() != 3 {
        println!("{}", "Invalid number of arguments to the command \"ifconfig\"");
        return -1
    }

    // Constructing the default config instruction packet
    let mut config_packet = nw_iface_config::default();

    // Checking the configuration network interface
    match arg_vec[0].as_ref(){
        "eth0"                  => {
            config_packet.set_iface(IFACE_ETH0);
        },
        "mirror_log_to_nw"      => {
            config_packet.set_iface(IFACE_MIRROR_LOG_TO_NW);
        },
        _                       => {
            println!("Invalid configuration network interface");
            return -1;
        },
    }

    // Checking the cmd type and argument to it
    let t_cmd = match parse_cmd (arg_vec[1].to_string()){
        Ok(cmd) => {
            config_packet.set_cmd(cmd);
            // Checking Ip address or port depending on the command type
            if cmd == SET_DESTINATION_IP || cmd == SET_SOURCE_IP{
                let t_ip = match parse_ip_address (arg_vec[2].to_string()){
                    Ok(ip) => {
                        config_packet.set_ip(ip);
                    }
                    Err(err) => {
                        println!("Error in arguments {}", err );
                    }
                };
            }
            else if cmd == SET_DESTINATION_PORT{
                let t_port = match parse_port_no (arg_vec[2].to_string()){
                    Ok(port) => {
                        config_packet.set_port(port);
                    }
                    Err(err) => {
                        println!("Error in arguments {}", err );
                    }
                };
            }

            else {
                println! ("Invalid command type");
                return -1; 
            }
        }
        Err(err) => {
            println!("Error in arguments {}", err );
        }
    };

    // Inserting the cmd arguments for the network processing queue
    add_nw_config_queue(config_packet);

    0
}


fn print_usage(opts: Options) -> isize {
    println!("{}", opts.usage(USAGE));
    0
}


const USAGE: &'static str = "Usage: example [ARGS]
An example application that just echoes its arguments.";