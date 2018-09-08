#![no_std]
#![feature(alloc)]

#![allow(dead_code)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate volatile;
extern crate owning_ref;
extern crate irq_safety;
extern crate atomic_linked_list;
extern crate memory;
extern crate spin;
extern crate kernel_config;






use core::ops::DerefMut;
use volatile::{Volatile, ReadOnly};
use alloc::boxed::Box;
use alloc::Vec;
use owning_ref::{BoxRef, BoxRefMut};
use spin::{Once, Mutex};
use memory::{FRAME_ALLOCATOR,Frame,ActivePageTable, PhysicalAddress, EntryFlags, MappedPages, allocate_pages};


//static CAPA_REGS: Once<BoxRef<MappedPages, CapabilityRegisters>> = Once::new();
//static OPRA_REGS: Once<BoxRefMut<MappedPages, OperationRegisters>> = Once::new();
static OPRA_REGS: Once<Mutex<BoxRefMut<MappedPages, OperationRegisters>>> = Once::new();

pub const EHCI_ADDRESS: u64 = 0xFEBF1000;


///Struct for defining usb interrupt type
///
pub enum IntType{

    AsyncAavance,
    HostControllerError,
    FrameListRollover,
    PortChange,
    UsbTransactionError,
    UsbTransactionComplete,

}



/// Read the current frame index, which is used to read the physical pointer
/// of current data struct
pub fn read_frame_index() -> Option<u32>{

    OPRA_REGS.try().map(|operation_reg| {

        operation_reg.lock().read_frame_index()

    })
}

/// Read the current frame index, which is used to read the physical pointer
/// of current data struct
pub fn read_micro_frame_index() -> Option<u8>{

    OPRA_REGS.try().map(|operation_reg| {

        operation_reg.lock().read_micro_frindex()

    })
}

/// Read the base address of the Periodic Schedule Frame List
pub fn read_framelist_base() -> Option<u32> {

    OPRA_REGS.try().map(|operation_reg| {

        operation_reg.lock().perodic_list_base.read()

    })

}

/// Read the base address of the Periodic Schedule Frame List
pub fn read_async_base() -> Option<u32> {

    OPRA_REGS.try().map(|operation_reg| {

        operation_reg.lock().asyn_list_base.read()

    })

}


/// Read the types of current interrupts by EHCI
/// Return a Vec of IntType
pub fn read_interrupt_type() -> Option<Vec<IntType>>{

    OPRA_REGS.try().map(|operation_reg| {

        let int_type_ids = (operation_reg.lock().usb_status.read() & 0x3F) as u8;
        let mut int_vec:Vec<IntType> =  Vec::new();
        let mut mask:u8 = 0x1;
        // read each bit to see the interrupt types
        for _x in 0..6{

            mask = mask << 1;
            let id:u8 = int_type_ids & mask;
            match id {

                0x20 => {

                    debug!("IntType::AsyncAavance");
                    int_vec.push(IntType::AsyncAavance);
                }

                0x10 => {

                    debug!("IntType::HostControllerError");
                    int_vec.push(IntType::HostControllerError);

                }

                0x08 => {

                   debug!("IntType::FrameListRollover");
                   int_vec.push(IntType::FrameListRollover);

                }

                0x04 => {
                    debug!("IntType::PortChange");
                    int_vec.push(IntType::PortChange);
                }

                0x02 => {

                    debug!("IntType::UsbTransactionError");
                    int_vec.push(IntType::UsbTransactionError);
                }

                0x01 => {

                    debug!("IntType::UsbTransactionComplete");
                    int_vec.push(IntType::UsbTransactionComplete);
                }

                _ => {}
            }
        }

        int_vec
    })


}

/// Acknowledge and handle the current interrupts accordign to the their type
pub fn interrupt_type_acknowledge(vec:Vec<IntType>) -> Option<()>{

    OPRA_REGS.try().map(|operation_reg| {

        for interrupt in vec {
            match interrupt {
                IntType::AsyncAavance => {
                    operation_reg.lock().deref_mut().async_advance_acknowledge();
                },
                IntType::UsbTransactionComplete => {
                    operation_reg.lock().deref_mut().trans_interrupt_acknowledge();
                },
                IntType::UsbTransactionError => {
                    operation_reg.lock().deref_mut().error_interrupt_acknowledge();
                },
                IntType::PortChange => {
                    operation_reg.lock().deref_mut().port_change_acknowledge();
                },
                IntType::FrameListRollover => {
                    operation_reg.lock().deref_mut().rollover_acknowledge();
                },
                IntType::HostControllerError => {
                    operation_reg.lock().deref_mut().host_error_acknowledge();
                },
            }
        }
    })


}

/// read the EHCI interrupt enable bits, which can tell whether
/// EHCI's interrupts are enabled
pub fn read_interrupt_enable() -> Option<u32>{

    OPRA_REGS.try().map(|operation_reg| {

        operation_reg.lock().interrupt_enable.read()

    })

}

/// initialize the EHCI, by mapping the Capability and Operation Registers and setting them
/// show the current configuration of the EHCI
pub fn init(active_table: &mut ActivePageTable) -> Result<(), &'static str> {

//    let capa_regs: BoxRef<MappedPages, CapabilityRegisters> = BoxRef::new(Box::new(map_capa_regs(active_table)?))
//        .try_map(|mp| mp.as_type::<CapabilityRegisters>(0))?;
    if let Ok(capa_regs) = box_capa_regs(active_table) {
        info!("\nHCIVERSION: {:x}\n", capa_regs.hci_version.read());
        info!("\nHCSPARAMS: {:x}\n", capa_regs.hcs_params.read());
        info!("\nHCCPARAMS: {:x}\n", capa_regs.hcc_params.read());
        info!("\nHCSP-port-route: {:x}\n", capa_regs.hcsp_portroute.read());
        let op_base = capa_regs.cap_length.read();
        info!("\nCAPALENGTH: {:x}\n", op_base);
        info!("\nPORTNUM: {:x}\n", capa_regs.host_ports_num());

        if let Ok(mut opra_register) = mut_box_op_regs(active_table, op_base) {
            {let opra_regs = & mut opra_register;

                //enable the USB interrupt
                opra_regs.usb_int(0x3F);
                opra_regs.set_interrupt_threshold(0x08);
            }

            if let Err(_e) =  opra_register.set_frame_size(0x10,capa_regs){

                debug!("fail to set USB frame size, now it is in defaul value");

            }

            info!("\nsee the data, USBCMD: {:x}\n", opra_register.usb_cmd.read());
            info!("\nsee the data, USBSTS: {:x}\n", opra_register.usb_status.read());
            info!("\nsee the data, USBINTR: {:x}\n", opra_register.interrupt_enable.read());
            info!("\nsee the data, FRINDEX: {:b}\n", opra_register.frame_index.read());
            info!("\nsee the data, CTRLDSSEGMENT: {:x}\n", opra_register.ctrlds_segment.read());
            info!("\nsee the data, PERIODICLISTBASE: {:x}\n", opra_register.perodic_list_base.read());
            info!("\nsee the data, ASYNLISTBASE: {:x}\n", opra_register.asyn_list_base.read());
            info!("\nsee the data, CONFIGFLAG: {:x}\n", opra_register.config_flag.read());
            info!("\nport 1: {:b}", opra_register.portsc.port_1.read());
            info!("\nport 2: {:b}", opra_register.portsc.port_2.read());
            info!("\nport 3: {:b}", opra_register.portsc.port_3.read());
            info!("\nport 4: {:b}", opra_register.portsc.port_4.read());
            info!("\nport 5: {:b}", opra_register.portsc.port_5.read());
            info!("\nport 6: {:b}", opra_register.portsc.port_6.read());
            OPRA_REGS.call_once(|| {
                Mutex::new(opra_register)
            });

            Ok(())
        } else {
            Err("Fail to read EHCI's operational register")
        }
    } else {
        Err("Fail to read EHCI's capability register")
    }
}


/// Box the Operation Registers with 'OperationRegisters' in Virtual Address
 pub fn box_capa_regs(active_table: &mut ActivePageTable)
                            -> Result<BoxRef<MappedPages, CapabilityRegisters> , &'static str>{

     let capa_regs: BoxRef<MappedPages, CapabilityRegisters> = BoxRef::new(Box::new(map_capa_regs(active_table)?))
         .try_map(|mp| mp.as_type::<CapabilityRegisters>(0))?;

     Ok(capa_regs)

 }

/// Box the Operation Registers with 'OperationRegisters' in Virtual Address as mutable
pub fn mut_box_op_regs(active_table: &mut ActivePageTable, op_base: u8)
                            -> Result<BoxRefMut<MappedPages, OperationRegisters>, &'static str>{

    let op_regs: BoxRefMut<MappedPages, OperationRegisters>  = BoxRefMut::new(Box::new(map_capa_regs(active_table)?))
        .try_map_mut(|mp| mp.as_type_mut::<OperationRegisters>(op_base as PhysicalAddress))?;

    Ok(op_regs)

}

/// Box the Capability Registers with 'CapabilityRegisters' in Virtual Address
pub fn mut_box_capa_regs(active_table: &mut ActivePageTable)
                         -> Result<BoxRefMut<MappedPages, CapabilityRegisters>, &'static str>{

    let op_regs: BoxRefMut<MappedPages, CapabilityRegisters>  = BoxRefMut::new(Box::new(map_capa_regs(active_table)?))
        .try_map_mut(|mp| mp.as_type_mut::<CapabilityRegisters>(0))?;

    Ok(op_regs)

}
/// return a mapping of EHCI Capability Registers
pub fn map_capa_regs(active_table: &mut ActivePageTable) -> Result<MappedPages, &'static str> {


    let phys_addr = EHCI_ADDRESS as PhysicalAddress;
    let new_page = try!(allocate_pages(1).ok_or("out of virtual address space for EHCI Capability Registers)!"));
    let frames = Frame::range_inclusive(Frame::containing_address(phys_addr), Frame::containing_address(phys_addr));
    let mut fa = try!(FRAME_ALLOCATOR.try().ok_or("EHCI::init(): couldn't get FRAME_ALLOCATOR")).lock();
    let capa_regs_mapped_page = try!(active_table.map_allocated_pages_to(
        new_page,
        frames,
        EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::NO_CACHE | EntryFlags::NO_EXECUTE,
        fa.deref_mut())
    );
    Ok(capa_regs_mapped_page)
}


/// struct to hold EHCI Capability Registers
#[repr(C)]
pub struct CapabilityRegisters {
    pub cap_length:                   ReadOnly<u8>,          // 0x00
    _padding1:                        u8,
    pub hci_version:                  ReadOnly<u16>,         // 0x02
    pub hcs_params:                   ReadOnly<u32>,         // 0x04
    pub hcc_params:                   ReadOnly<u32>,         // 0x08
    pub hcsp_portroute:               ReadOnly<u64>,         // 0x0C
}

/// struct to hold EHCI Operation Registers
#[repr(C)]
pub struct OperationRegisters {
    pub usb_cmd:                      Volatile<u32>,         // 0x00
    pub usb_status:                   Volatile<u32>,         // 0x04
    pub interrupt_enable:             Volatile<u32>,         // 0x08
    pub frame_index:                  Volatile<u32>,         // 0x0C
    pub ctrlds_segment:               Volatile<u32>,         // 0x10
    pub perodic_list_base:            Volatile<u32>,         // 0x14
    pub asyn_list_base:               Volatile<u32>,         // 0x18
    _padding1:                        [u32;9],               // 0x1C-0x3F
    pub config_flag:                  Volatile<u32>,         // 0x40
    pub portsc:                       PortStatusAndControl   // 0x44
}

/// struct to hold USB ports' configuration pointers
#[repr(C)]
pub struct PortStatusAndControl {
    pub port_1:                      Volatile<u32>,         // 0x44
    pub port_2:                      Volatile<u32>,         // 0x48
    pub port_3:                      Volatile<u32>,         // 0x4C
    pub port_4:                      Volatile<u32>,         // 0x50
    pub port_5:                      Volatile<u32>,         // 0x54
    pub port_6:                      Volatile<u32>,         // 0x58
}

impl CapabilityRegisters{

    /// read the offset between Capability Registers and Opertion Registers
    pub fn read_capa_length(&self)-> u8{
        self.cap_length.read()
    }

    /// read the number of companion Host Controllers
    pub fn companion_controller_num(&self)-> u8{
        let value = self.hcs_params.read();
        let num = ((value & 0xF000) >> 12) as u8;
        num
    }

    /// read the number of ports per companion Host Controllers
    pub fn ports_per_cc(&self)-> u8{
        let value = self.hcs_params.read();
        let num = ((value & 0xF00) >> 8) as u8;
        num
    }

    /// read the number of ports in EHCI
    pub fn host_ports_num(&self)-> u8{
        let value = self.hcs_params.read();
        let num = (value & 0xF) as u8;
        num
    }


    /// see whether the host controller has the power control
    pub fn port_indicator(&self)-> bool{
        let value = self.hcs_params.read();
        let num = (value & 0x10000) >> 16;
        num == 1
    }

    /// read the pointer to EHCI Extended Capabilities
    pub fn offset_for_exteneded_capability(&self) -> u8{
        let value = self.hcc_params.read();
        let num = ((value & 0xFF00) >> 8) as u8;
        num
    }

    ///read the Isochronous Scheduling Threshold
    pub fn isochro_schedule_threshold(&self) -> u8{
        let value = self.hcc_params.read();
        let num = ((value & 0xF0) >> 4) as u8;
        num
    }

    /// see whether the ECHI has the Asynchronous Schedule Park Capablity
    pub fn asyn_schedule_park(&self) -> bool{
        let value = self.hcc_params.read();
        let num = (value & 0x4) >> 2;
        num == 1
    }

    /// see whether the EHCI's frame list size is programmable
    /// If false, the EHCI's frame list size is set to 1024 elements
    pub fn promgrammbale_frame_list_flag(&self) -> bool{
        let value = self.hcc_params.read();
        let num = (value & 0x2) >> 1;
        num == 1
    }


}

impl OperationRegisters{



    /// set the maximum rate at which the host controller issues interrupts
    /// 00h: Reserved, 01h: 1 micro-frame, 02h: 2 micro-frames, 04: 4 micro-frames
    /// 08h: 8 micro-frames, 10h: 16 micro-frames, 20h: 32 micro-frames
    /// 40h: 64 micro-frames.
    pub fn set_interrupt_threshold(&mut self, value:u8){
        let command = (value as u32) << 16;
        self.usb_cmd.update(|old_val_ref| *old_val_ref |= command);
    }

    /// set the Size of the micro-frames
    /// 00b: 4096 bytes, 01b: 2048 bytes, 10b: 1024 bytes, 11: reserved.
    pub fn set_frame_size(&mut self, value: u8, capa_reg: BoxRef<MappedPages, CapabilityRegisters>) -> Result<(),&'static str>{
        if capa_reg.promgrammbale_frame_list_flag(){
            let command = (value as u32) << 2;
            self.usb_cmd.update(|old_val_ref| *old_val_ref |= command);
            Ok(())
        }else{
            Err("The Frame-size is not programmable. Fail to set size")
        }

    }

    /// Set the periodic schedule
    /// 0: disable, 1: enable
    pub fn set_periodic_schedule(&mut self, value: u8){
        let command = (value as u32) << 4;
        self.usb_cmd.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Set the host controller reset bit
    /// 0: finish reset , 1: reset
    pub fn reset_host_controller(&mut self, value: u8){
        let command = (value << 1) as u32;
        self.usb_cmd.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Run/Stop the Host Controller
    /// 0: Stop, 1: Run
    pub fn run_or_stop(&mut self, value: u8){
        let command = value as u32;
        self.usb_cmd.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Acknowledge the Asynchronous Advance Interrupt
    pub fn async_advance_acknowledge(&mut self){
        let command = (1 as u32) << 5;
        self.usb_status.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Acknowledge the Host Controller Error Interrupt
    pub fn host_error_acknowledge(&mut self){
        let command = (1 as u32) << 4;
        self.usb_status.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Acknowledge the Framelist Rollover Interrupt
    pub fn rollover_acknowledge(&mut self){
        let command = (1 as u32) << 3;
        self.usb_status.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Acknowledge the USB ports' changes Interrupt
    pub fn port_change_acknowledge(&mut self){
        let command = (1 as u32) << 2;
        self.usb_status.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Acknowledge the Transactions' errors Interrupt
    pub fn error_interrupt_acknowledge(&mut self){
        let command = (1 as u32) << 1;
        self.usb_status.update(|old_val_ref| *old_val_ref |= command);
    }

    /// Acknowledge the Transaction triggered Interrupt
    pub fn trans_interrupt_acknowledge(&mut self){
        let command = 1 as u32;
        self.usb_status.update(|old_val_ref| *old_val_ref |= command);
    }

    /// enable/disable the Host Controller error interrupt
    /// 1: enbale, 0: disable
    pub fn host_system_error(&mut self, value: u8){
        let command = (value as u32) << 4;
        self.interrupt_enable.update(|old_val_ref| *old_val_ref |= command);
    }

    /// enable/disable the Host Controller error interrupt
   /// 1: enbale, 0: disable
    pub fn frame_list_rollover(& mut self, value: u8){
        let command = (value as u32) << 3;
        self.interrupt_enable.update(|old_val_ref| *old_val_ref |= command);
    }

    /// enable/disable the Ports' Changes interrupt
    /// 1: enbale, 0: disable
    pub fn port_change_int(&mut self, value: u8){
        let command = (value as u32) << 2;
        self.interrupt_enable.update(|old_val_ref| *old_val_ref |= command);
    }

    /// enable/disable the Transaction error interrupt
    /// 1: enbale, 0: disable
    pub fn usb_error_int(&mut self, value: u8){
        let command = (value as u32) << 1;
        self.interrupt_enable.update(|old_val_ref| *old_val_ref |= command);
    }

    /// enable/disable the Transaction Completion error interrupt
    /// 1: enbale, 0: disable
    pub fn usb_int(&mut self, value: u8){
        let command = value as u32;
        self.interrupt_enable.update(|old_val_ref| *old_val_ref |= command);
    }

    /// set the port-routing control logic
    /// 0: route each port to an implementation dependent classc Host Controller
    /// 1: route all ports EHCI
    pub fn set_config_flag(&mut self, value: u8){
        let command = value as u32;
        self.config_flag.update(|old_val_ref| *old_val_ref |= command);
    }

    /// read the current frame index
    pub fn read_frame_index(&self) -> u32{

        let frameindex = (self.frame_index.read() & 0x1FF8) >> 1;
        frameindex
    }

    /// read the micro frame index
    pub fn read_micro_frindex(&self) -> u8{

        let microframeindex = (self.frame_index.read() & 0x7) as u8;
        microframeindex
    }

    /// read the reclamation bit, which is used to see whether the Async Schedule is empty
    pub fn read_reclamation(&self) -> u8{

        let rec = ((self.usb_status.read() & 0x2000) >> 13) as u8;
        rec
    }

    /// update the reclamation bit to 1 or 0
    pub fn update_reclamation(& mut self, value: u32){

        if value == 1{

            let value_s = value << 13;
            self.usb_status.update(|old_status| *old_status |= value_s );

        }
        if value == 0{

            self.usb_status.update(|old_status| *old_status &= 0xFFFFDFFF);
        }
    }


}





