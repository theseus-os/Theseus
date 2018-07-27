use smoltcp::wire::IpAddress;
use alloc::string::String;

/// Type aliases
pub type PortNumber     = u16;
pub type CommandType    = u8;

/// Interface to configure
pub const  IFACE_ETH0               :u8 = 0;
pub const  IFACE_MIRROR_LOG_TO_NW   :u8 = 1;

/// Command types
pub const  SET_DESTINATION_IP       :u8 = 0;
pub const  SET_DESTINATION_PORT     :u8 = 1;
pub const  SET_SOURCE_IP              :u8 = 2;
pub const  SET_SOURCE_PORT            :u8 = 3;

/// struct store commands to configure the network interface
pub struct nw_iface_config {
    // Interface to configure
    iface       :u8,
    // Type of command
    cmd         :CommandType,
    // Ip address
    ip          :IpAddress,
    // Port number
    port        :PortNumber
}

impl nw_iface_config{
    pub fn new(iface:u8, cmd:CommandType, ip:IpAddress, port:PortNumber) -> nw_iface_config {
        nw_iface_config {
            iface:iface,
            cmd:cmd,
            ip:ip,
            port:port,
        }
    } 

    pub fn default() -> nw_iface_config {
        nw_iface_config {
            iface:0,
            cmd:0,
            ip:IpAddress::v4(0, 0, 0, 0),
            port:0,
        }
    }
    // setter functions
    pub fn set_iface(&mut self, iface:u8){
        self.iface = iface;
    }

    pub fn set_cmd(&mut self, cmd:CommandType){
        self.cmd = cmd;
    }

    pub fn set_ip(&mut self, ip:IpAddress){
        self.ip = ip;
    }

    pub fn set_port(&mut self, port:PortNumber){
        self.port = port;
    }
    // getter functions
    pub fn get_iface(&self)->u8{
        self.iface 
    }

    pub fn get_cmd(&self)->CommandType{
        self.cmd
    }

    pub fn get_ip(&self)->IpAddress{
        self.ip 
    }

    pub fn get_port(&self)->PortNumber{
        self.port
    }
}


/// Parse iface config command
pub fn parse_cmd (command:String) -> Result<CommandType, &'static str>{
    match command.as_ref(){
        "set_destination_ip"        => Ok(SET_DESTINATION_IP),
        "set_destination_port"      => Ok(SET_DESTINATION_PORT),
        "set_source_ip"             => Ok(SET_SOURCE_IP),
        "set_source_port"           => Ok(SET_SOURCE_PORT),
        _                           => Err("Invalid command"),
    }
}


/// Convert string to and IPv4 address
pub fn parse_ip_address (addr:String) -> Result<IpAddress, &'static str>{
	let mut ip: [u8; 4] = [0;4];
	let split = addr.split(".");
	if split.clone().count()!= 4 {
		return Err("Invalid IP address")
	}
	let mut x_count = 0;
	for x in split{
		match x.parse::<u8>(){
			Ok(y) => {
				ip[x_count] = y;
				x_count = x_count + 1;
			}
			_ => {
				return Err("Invalid IP address")
			}
		}
	}
	Ok (IpAddress::v4(ip[0],ip[1],ip[2],ip[3]))

}

/// Convert string to a Port number
pub fn parse_port_no (port_no:String) -> Result<PortNumber, &'static str>{
    match port_no.parse::<PortNumber>(){
        Ok(y) => {
            Ok(y)
        }
        _ => {
            return Err("Invalid Port Number")
        }
    }

}