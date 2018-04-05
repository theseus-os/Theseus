
use alloc::slice;
use alloc::vec::Vec;
use smoltcp::Error;
use smoltcp::phy::Device;
use e1000::E1000_NIC;

const TX_BUFFERS: [*mut u8; 2] = [0x0 as *mut u8, 0x0 as *mut u8];
const RX_BUFFERS: [*mut u8; 2] = [0x0 as *mut u8, 0x0 as *mut u8];


fn rx_full() -> bool {
    /* platform-specific code to check if an incoming packet has arrived */
    let mut e1000_nc = E1000_NIC.lock();
    e1000_nc .has_packet_arrived()
}

fn rx_setup() -> (*mut u8, usize) {
    /* platform-specific code to receive a packet into a buffer */
    // let mut e1000_nc = E1000_NIC.lock();
    // let len:usize;
    // let (mut buf2,mut len) =  e1000_nc.receive_single_packet2();
    // length = &mut len;
    let mut e1000_nc = E1000_NIC.lock();
    e1000_nc.receive_single_packet2()
}
// fn rx_setup() -> (*mut u8, usize) {
//     let mut e1000_nc = E1000_NIC.lock();
//     e1000_nc.receive_single_packet3()
// }

fn tx_empty() -> bool {
    /* platform-specific code to check if the outgoing packet was sent */
    //false
    // let mut e1000_nc = E1000_NIC.lock();
    // debug!("is tx_empty {}", e1000_nc.has_packet_sent());
    // e1000_nc.has_packet_sent()
    true
}

fn tx_setup(buf: *const u8, length: usize) {
    /* platform-specific code to send a buffer with a packet */
    unsafe {debug!("SeNDINg {:?}", slice::from_raw_parts(buf, length));}
    let addr: usize = buf as usize;
    let mut e1000_nc = E1000_NIC.lock();
    let _result = e1000_nc.send_packet(addr, length as u16);
}

pub struct EthernetDevice{
    pub tx_next: usize,
    pub rx_next: usize
}

impl Device for EthernetDevice {
    type RxBuffer = &'static [u8];
    type TxBuffer = TxBuffer;

    fn mtu(&self) -> usize { 1536 }

    fn receive(&mut self) -> Result<Self::RxBuffer, Error> {
        if rx_full() {
            let index = self.rx_next;
            self.rx_next = (self.rx_next + 1) % RX_BUFFERS.len();
            
            let (buf, length) = rx_setup();
            Ok(unsafe {
                slice::from_raw_parts(buf, length)
            })
        } else {
            Err(Error::Exhausted)
        }
    }

    fn transmit(&mut self, length: usize) -> Result<Self::TxBuffer, Error> {
        // if tx_empty() {
        //     Ok(TxBuffer {
        //         buffer: vec![0; length],
        //         length: length
        //     })
        // } else {
        //     Err(Error::Exhausted)
        // }

        debug!("length: {}", length );
        debug!("address {:?}", TX_BUFFERS[self.tx_next] );
        if tx_empty() {
            let index = self.tx_next;
            self.tx_next = (self.tx_next + 1) % TX_BUFFERS.len();
            debug!("length: {}", length);
            // Ok(EthernetTxBuffer(unsafe {

            //     //debug!("$$$$ transmit {:?}", slice::from_raw_parts_mut(TX_BUFFERS[index], length));
            //     slice::from_raw_parts_mut(TX_BUFFERS[index], length)
            // }))
            //let tmp = vec![0;length];
            Ok(TxBuffer {
               buffer : vec![0;length],
            })
        } else {
            Err(Error::Exhausted)
        }
    }
}

pub struct TxBuffer {
    //lower:  Rc<RefCell<sys::TapInterfaceDesc>>,
    buffer: Vec<u8>
}

impl AsRef<[u8]> for TxBuffer {
    fn as_ref(&self) -> &[u8] { self.buffer.as_ref() }
}

impl AsMut<[u8]> for TxBuffer {
    fn as_mut(&mut self) -> &mut [u8] { self.buffer.as_mut() }
}

impl Drop for TxBuffer {

    fn drop(&mut self) { 
        trace!("**** BUFFER TX SETUP - {:?}", self.buffer);
        tx_setup(self.buffer.as_ptr(), self.buffer.len()) 
    }

}


//pub struct EthernetTxBuffer(&'static mut [u8]);
//pub struct EthernetTxBuffer(& mut [u8]);

// impl AsRef<[u8]> for EthernetTxBuffer {
//     fn as_ref(&self) -> &[u8] { self.0 }
// }

// impl AsMut<[u8]> for EthernetTxBuffer {
//     fn as_mut(&mut self) -> &mut [u8] { self.0 }
// }

// impl Drop for EthernetTxBuffer {
//     fn drop(&mut self) { tx_setup(self.0.as_ptr(), self.0.len()) }
// }

/*
// #[doc(hidden)]
pub struct TxBuffer<'a> {
    buffer: &'a mut [u8],
    //length: usize
}

impl<'a> AsRef<[u8]> for TxBuffer<'a> {
    fn as_ref(&self) -> &[u8] { self.buffer.as_ref() }
}

impl<'a> AsMut<[u8]> for TxBuffer<'a> {
    fn as_mut(&mut self) -> &mut [u8] { self.buffer.as_mut() }
}

impl<'a> Drop for TxBuffer<'a> {

    fn drop(&mut self) { tx_setup(self.buffer.as_ptr(), self.buffer.len()) }

}*/

// pub fn array_from_vec(packet: &[u8]) -> [u8;1536]{
//         let mut a = [0;1536];
//         for i in 0..packet.len(){
//                 a[i]   = packet[i];
//         }
//         a
// }
