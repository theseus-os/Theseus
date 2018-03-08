
use alloc::string::String;
use alloc::{BTreeMap, VecDeque};
use irq_safety::{RwLockIrqSafe, RwLockIrqSafeReadGuard, RwLockIrqSafeWriteGuard};
use core::ops::DerefMut;
use spin::{Once, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use alloc::arc::Arc;
use util::c_str::{c_char, CStr};




#[derive(Clone)]
pub struct BusMessage {    
    pub dest: String,
    pub data: String,
}

impl BusMessage {
    pub fn new(dest:String, data: String) -> BusMessage {
        BusMessage {
            dest:dest,
            data:data,
        }
    }
}


pub struct BusConnection {
    pub name: String,
     
    pub refcount: u32,
    
    //
    //lock
    //
    
    pub outgoing: RwLock<VecDeque<BusMessage>>,
    
    pub incoming: RwLock<VecDeque<BusMessage>>,
    
    pub outnum: u32,
    
    pub innum: u32,
}


impl BusConnection {
    fn new(busName:&String) -> BusConnection {
        BusConnection {
            name:String::clone(busName),
            refcount:1,
            outgoing:RwLock::new(VecDeque::new()),
            incoming:RwLock::new(VecDeque::new()),
            outnum:0,
            innum:0,
        }
    }

    pub fn send(&mut self, buf:&BusMessage) {
        let message = BusMessage::clone(buf);
        self.outgoing.write().push_front(message);
        self.outnum+=1;
    }

    pub fn receive(&mut self) -> Option<BusMessage> {
        self.incoming.write().pop_back()
    }

    
}

pub struct BusConnectionTable {
    table: BTreeMap<String, Arc<RwLock<BusConnection>>>,
    count:u32,
}

impl BusConnectionTable {
    pub fn new() -> BusConnectionTable {
        BusConnectionTable {
            table: BTreeMap::new(),
            count:1,
        }
    }

    pub fn get_connection(&mut self, name:String) ->  Option<&Arc<RwLock<BusConnection>>> {
        //let mut conn:&mut BusConnection;
        let connectionName = String::clone(&name);
        if !self.table.contains_key(&name){
            let connection = BusConnection::new(&name);
            self.table.insert(name, Arc::new(RwLock::new(connection)));
            self.count+=1;
        }
        return self.table.get(&connectionName);
    }

    pub fn match_msg(&self, name:&String){
       
        let mut source = self.table.get(name).expect("Fail to get the source connection").write();
        let msg_obj = source.outgoing.write().pop_back();
        
        if msg_obj.is_some(){
            let msg = msg_obj.expect("Fail to get the message");
            let dest_obj = self.table.get(&msg.dest);
            if(dest_obj.is_some()){
                let mut destination = dest_obj.expect("Fail to get the destination connection").write();
                destination.incoming.write().push_front(msg);
                destination.innum += 1;
                println!("Send the message successfully!");
            } else {
                source.outgoing.write().push_front(msg);
                println!("Destination connection does not exist!");
            }
            source.outnum -= 1;
        }

    }

  /*  pub fn get_conn(name:String) ->  Option<&'static Arc<RwLock<BusConnection>>> {
     None
        
    }*/
}

static CONNECTION_TABLE: Once<RwLockIrqSafe<BusConnectionTable>> = Once::new();

pub fn get_connection_table() -> &'static RwLockIrqSafe<BusConnectionTable> {
    
    CONNECTION_TABLE.call_once( || {
        RwLockIrqSafe::new(BusConnectionTable::new())
    })
}

pub fn syssend(src:&CStr, dest:&CStr, msg:&CStr) {
    let mut table = get_connection_table().write();
    let ssrc = src.to_string_lossy().into_owned();

    let sdest = dest.to_string_lossy().into_owned();
    trace!("Create the connection {}", &ssrc);
    trace!("Send msg to connection {}", &sdest);
    {

            let mut connection = table.get_connection(String::clone(&ssrc))
                .expect("Fail to create the destination bus connection").write();
            let message =  BusMessage::new(sdest, msg.to_string_lossy().into_owned());
            connection.send(&message);           
    }
    

    table.match_msg(&ssrc);

}

pub fn sysrecv(name:&CStr) -> String{
    let sname = name.to_string_lossy().into_owned();;;
    unsafe{
        trace!("Get the connection {}", &sname);
        let mut table = get_connection_table().write();
        let mut connection = table.get_connection(sname)
            .expect("Fail to create the bus connection").write();
  
        let obj = connection.receive();
        if(obj.is_some()){
            trace!("Get the result!");
            return obj.unwrap().data;
        } else {
            trace!("No message!");
            return String::from("");
        }
    }
}
