use cpuio::Port;
use spin::Mutex;

const SERIAL_PORT_COM1: u16 = 0x3F8;
const SERIAL_PORT_COM1_READY: u16 = SERIAL_PORT_COM1 + 5;
const SERIAL_PORT_READY_MASK: u8 = 0x20;

static serial_com1: Mutex<Port<u8>> = Mutex::new( unsafe { Port::new(SERIAL_PORT_COM1) } );
static serial_com1_ready: Mutex<Port<u8>> = Mutex::new( unsafe { Port::new(SERIAL_PORT_COM1_READY) } );

//struct SerialPort {
//	port: Port<u8>
//}
//
//impl SerialPort {
//	
//}

pub fn serial_out(s: &str) {
	//&* is necessary:  * is required to dereference the MutexGuard obtained from .lock() above,
	//                  & is required to borrow the Port<u8> object as a reference
	let mut serial_com1_locked_mutex_guard = serial_com1.lock();
	let locked = &mut *serial_com1_locked_mutex_guard;
	
	for b in s.bytes() {
		write_locked(locked, b); 
	}
	
}

fn write_locked(locked_port: &mut Port<u8>, b: u8) {
	wait_for_ready();
	locked_port.write(b);
}


fn wait_for_ready() {
	while serial_com1_ready.lock().read() & SERIAL_PORT_READY_MASK == 0 {
		// do nothing
	}
}