
use collections::string::String;
use collections::{BTreeMap, VecDeque};

#[derive(Clone)]
pub struct BusMessage {    
    pub data: String,    
}

#[derive(Clone)]
pub struct BusConnection {
    pub name: String,
     
    pub refcount: u32,
    
    //
    //lock
    //
    
    pub outgoing: VecDeque<BusMessage>,
    
    pub incoming: VecDeque<BusMessage>,
    
    pub outnum: u32,
    
    pub innum: u32,
}


impl BusConnection {
    fn new(busName:&String) -> BusConnection {
        BusConnection {
            name:String::clone(busName),
            refcount:0,
            outgoing:VecDeque::new(),
            incoming:VecDeque::new(),
            outnum:0,
            innum:0,
        }
    }
}

pub struct BusConnectionTable {
    table: BTreeMap<String, BusConnection>,
    count:u32,
}

impl BusConnectionTable {
    pub fn new() -> BusConnectionTable {
        BusConnectionTable {
            table: BTreeMap::new(),
            count:1,
        }
    }

    /// returns a shared reference to the current `Task`
    pub fn get_connection(&mut self, name:String) -> &BusConnection {
        //let mut conn:&mut BusConnection;
        let connectionName = String::clone(&name);
        if !self.table.contains_key(&name){
            let connection = BusConnection::new(&name);
            self.table.insert(name, connection);
            self.count+=1;
        }
        {
            let obj:Option<&mut BusConnection>;
            obj = self.table.get_mut(&connectionName);
            //if obj.is_some(){
                let conn = obj.expect("Fail to get the connection");
                conn.refcount+=1;
                return conn;
            //}
        }     
    }
}

    