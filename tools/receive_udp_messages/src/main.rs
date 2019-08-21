
use std::net::UdpSocket;
use std::str;

fn main() {  
	let socket = UdpSocket::bind("192.168.69.100:5901").expect("couldn't bind to address");
	let s = b"abcdef";
	let mut i = 0;
	while i < 30 {
		let mut buf = [0; 100];
		let (number_of_bytes, src_addr) = socket.recv_from(&mut buf).expect("Didn't receive data");
		let filled_buf = &mut buf[..number_of_bytes];

		let mut s = str::from_utf8(filled_buf);
		match s {
        	Result::Ok(s1) => println!("{}",s1),
            Result::Err(err) => (),
        }

		i = i +1;
	}
}
