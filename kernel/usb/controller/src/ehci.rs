#![allow(dead_code)]

use super::*;

pub fn init(ehci_pci_dev: &PciDevice) -> Result<(), &'static str> {
    let base = (ehci_pci_dev.bars[0] as usize) & !0xff;
    log::error!("Mapping 0x{:x} as EHCI", base);
    let base = PhysicalAddress::new(base).ok_or("Invalid PCI BAR for EHCI USB controller")?;

    let (mut alloc, four_gig_segment) = UsbAlloc::new(24, 24, 24, 24)?;

    let mut mapped_pages = map_frame_range(base, PAGE_SIZE, MMIO_FLAGS)?;
    let capa_regs = mapped_pages.as_type::<CapabilityRegisters>(0)?;

    let op_offset = capa_regs.cap_length.read() as usize;
    let hcs_params = capa_regs.hcs_params.read();

    let op_regs = mapped_pages.as_type_mut::<OperationRegisters>(op_offset)?;

    op_regs.segment.write(four_gig_segment);
    op_regs.command.update(|cmd| cmd.set_async_schedule(false));
    op_regs.command.update(|cmd| cmd.set_periodic_schedule(false));
    op_regs.command.update(|cmd| cmd.set_run_stop(true));
    op_regs.config_flag.update(|cmd| cmd.set_inner(ConfigureFlag::Use));

    sleep(Duration::from_millis(10)).unwrap();

    log::error!("Initializing an EHCI USB controller with {} ports and {} companion controllers",
        hcs_params.port_num(),
        hcs_params.comp_ctrl_num());

    let port_num = hcs_params.port_num().value() as usize;
    for i in 0..port_num {
        let port = &mut op_regs.ports[i];
        if port.read().connected() {
            // reset the device; it will now reply to requests targeted at address zero
            port.update(|port| port.set_port_state(false));
            port.update(|port| port.set_port_reset(true));
            sleep(Duration::from_millis(10)).unwrap();
            port.update(|port| port.set_port_reset(false));
            log::error!("CONNECTED: {:#?}", port.read());
        }
    }

    let data_sz = size_of::<DeviceDescriptor>();

    let req = Request::new(
        RequestRecipient::Device,
        RequestType::Standard,
        Direction::In,
        RequestName::GetDescriptor,
        0x0100,
        0,
        data_sz as _,
    );

    let (desc_offset, desc) = alloc.alloc_desc(DeviceDescriptor::default())?;

    let (qh, qh_index, setup, status) = create_read_request(&mut alloc, 0, req, data_sz, 8, desc)?;

    op_regs.async_list.write(qh);
    op_regs.command.update(|cmd| cmd.set_async_schedule(true));

    loop {
        let setup_qtd = alloc.get_qtd_mut(setup)?;
        if setup_qtd.token.read().active() {
            sleep(Duration::from_millis(10)).unwrap();
        } else {
            break;
        }
    }

    loop {
        let setup_qtd = alloc.get_qtd_mut(status)?;
        if setup_qtd.token.read().active() {
            sleep(Duration::from_millis(10)).unwrap();
        } else {
            break;
        }
    }

    op_regs.command.update(|cmd| cmd.set_async_schedule(false));

    let descriptor = alloc.get_desc_mut(desc_offset)?;
    log::warn!("DESCRIPTOR: {:#x?}", *descriptor);

    Ok(())
}

fn create_read_request(
    alloc: &mut UsbAlloc,
    dev_addr: u8,
    req: Request,
    mut data_size: usize,
    max_packet_size: usize,
    mut in_buf: u32,
) -> Result<(u32, usize, usize, usize), &'static str> {
    let (_, req_ptr) = alloc.alloc_req(req)?;
    let req_bp = BufferPointerWithOffset::from(req_ptr);

    let zero_bp = BufferPointer::from(0);
    let no_next = PointerNoType::from(1);
    let mut data_toggle = false;

    let setup_token = QtdToken::new(
        // initial status flags:
        false, false, false, false, false, false, false,

        true,
        PidCode::Setup,
        u2::new(3),
        u3::new(0),
        true,
        u15::new(size_of::<Request>() as _),
        data_toggle,
    );

    let setup_qtd = TransferDescriptor {
        next: Volatile::new(no_next),
        alt_next: Volatile::new(no_next),
        token: Volatile::new(setup_token),
        bp0: Volatile::new(req_bp),
        bp1: Volatile::new(zero_bp),
        bp2: Volatile::new(zero_bp),
        bp3: Volatile::new(zero_bp),
        bp4: Volatile::new(zero_bp),
    };

    let (first_qtd_offset, first_qtd_ptr) = alloc.alloc_qtd(setup_qtd)?;
    let mut prev_qtd_offset = first_qtd_offset;

    while data_size > 0 {
        let progress = data_size.min(max_packet_size);

        let in_token = QtdToken::new(
            // initial status flags:
            false, false, false, false, false, false, false,

            true,
            PidCode::In,
            u2::new(3),
            u3::new(0),
            true,
            u15::new(progress as _),
            data_toggle,
        );

        let in_buf_bp = BufferPointerWithOffset::from(in_buf);

        let in_qtd = TransferDescriptor {
            next: Volatile::new(no_next),
            alt_next: Volatile::new(no_next),
            token: Volatile::new(in_token),
            bp0: Volatile::new(in_buf_bp),
            bp1: Volatile::new(zero_bp),
            bp2: Volatile::new(zero_bp),
            bp3: Volatile::new(zero_bp),
            bp4: Volatile::new(zero_bp),
        };

        let (part_qtd_offset, part_qtd_ptr) = alloc.alloc_qtd(in_qtd)?;

        let prev_qtd = alloc.get_qtd_mut(prev_qtd_offset)?;
        prev_qtd.next.write(PointerNoType::from(part_qtd_ptr));

        prev_qtd_offset = part_qtd_offset;

        in_buf += progress as u32;
        data_size -= progress;
        data_toggle = !data_toggle;
    }

    let status_token = QtdToken::new(
        // initial status flags:
        false, false, false, false, false, false, false,
        true,
        PidCode::Out,
        u2::new(3),
        u3::new(0),
        true,
        u15::new(0),
        // force to true?
        data_toggle,
    );

    let status_qtd = TransferDescriptor {
        next: Volatile::new(no_next),
        alt_next: Volatile::new(no_next),
        token: Volatile::new(status_token),
        bp0: Volatile::new(BufferPointerWithOffset::from(0)),
        bp1: Volatile::new(zero_bp),
        bp2: Volatile::new(zero_bp),
        bp3: Volatile::new(zero_bp),
        bp4: Volatile::new(zero_bp),
    };

    let (status_qtd_offset, status_qtd_ptr) = alloc.alloc_qtd(status_qtd)?;

    let prev_qtd = alloc.get_qtd_mut(prev_qtd_offset)?;
    prev_qtd.next.write(PointerNoType::from(status_qtd_ptr));

    let qh_endpoint = QhEndpoint::new(
        u7::new(dev_addr),
        false,
        u4::new(0),
        EndpointSpeed::HighSpeed,
        true,
        true,
        u11::new(max_packet_size as _),
        true,
        u4::new(0),
    );

    let qh_uframe = QhMicroFrame::new(
        0, 0, u7::new(0), u7::new(0),
        HighBandwidthPipeMultiplier::One,
    );

    let first_qtd_bp = PointerNoType::from(first_qtd_ptr);

    let qh = QueueHead {
        next: Volatile::new(Pointer::from(1)),
        reg0: Volatile::new(qh_endpoint),
        reg1: Volatile::new(qh_uframe),
        current_qtd: Volatile::new(PointerNoTypeNoTerm::from(0)),

        // Transfer Overlay
        next_qtd: Volatile::new(first_qtd_bp),
        alt_next_qtd: Volatile::new(AltNextQtdPointer::from(0)),
        token: Volatile::new(QtdToken::from(0)),
        bp0: Volatile::new(BufferPointerWithOffset::from(0)),
        bp1: Volatile::new(QhBp1::from(0)),
        bp2: Volatile::new(QhBp2::from(0)),
        bp3: Volatile::new(BufferPointer::from(0)),
        bp4: Volatile::new(BufferPointer::from(0)),
    };

    let (qh_offset, queue_head) = alloc.alloc_qh(qh)?;

    // circular queue
    let qh_mut = alloc.get_qh_mut(qh_offset)?;
    qh_mut.next.write(Pointer::new(false, PointerType::QueueHead, u27::new(queue_head >> 5)));

    Ok((queue_head, qh_offset, first_qtd_offset, status_qtd_offset))
}

struct UsbAlloc {
    req_obj_offset: usize,
    req_offset: usize,
    req_slots: usize,

    qh_obj_offset: usize,
    qh_offset: usize,
    qh_slots: usize,

    qtd_obj_offset: usize,
    qtd_offset: usize,
    qtd_slots: usize,

    desc_obj_offset: usize,
    desc_offset: usize,
    desc_slots: usize,

    in_use: Vec<bool>,
    bytes: MappedPages,
}

impl UsbAlloc {
    /// Returns (self, four_gig_segment)
    pub fn new(
        req_slots: usize,
        qh_slots: usize,
        qtd_slots: usize,
        desc_slots: usize,
    ) -> Result<(Self, u32), &'static str> {
        let mut total_size = 0;
        let mut total_objs = 0;

        let req_offset = total_size;
        let req_obj_offset = total_objs;
        total_size += req_slots * size_of::<Request>();
        total_objs += req_slots;

        let qh_offset = total_size;
        let qh_obj_offset = total_objs;
        total_size += qh_slots * size_of::<QueueHead>();
        total_objs += qh_slots;

        let qtd_offset = total_size;
        let qtd_obj_offset = total_objs;
        total_size += qtd_slots * size_of::<TransferDescriptor>();
        total_objs += qtd_slots;

        let desc_offset = total_size;
        let desc_obj_offset = total_objs;
        total_size += desc_slots * size_of::<DeviceDescriptor>();
        total_objs += desc_slots;

        // todo: make sure this doesn't cross a 4GiB boundary
        let num_pages = (total_size + (PAGE_SIZE - 1)) / PAGE_SIZE;
        let bytes = create_identity_mapping(num_pages, MMIO_FLAGS)?;
        let four_gig_segment = (bytes.start_address().value() >> 32) as u32;

        let this = Self {
            req_obj_offset,
            req_offset,
            req_slots,

            qh_obj_offset,
            qh_offset,
            qh_slots,

            qtd_obj_offset,
            qtd_offset,
            qtd_slots,

            desc_obj_offset,
            desc_offset,
            desc_slots,

            in_use: vec![false; total_objs],
            bytes,
        };

        Ok((this, four_gig_segment))
    }

    pub fn alloc_req(&mut self, req: Request) -> Result<(usize, u32), &'static str> {
        for i in 0..self.req_slots {
            let obj = self.req_obj_offset + i;
            if !self.in_use[obj] {
                self.in_use[obj] = true;
                let offset = self.req_offset + i * size_of::<Request>();
                let mut_ref = self.bytes.as_type_mut(offset)?;
                *mut_ref = req;
                let addr = mut_ref as *const Request as usize;

                return Ok((offset, addr as u32))
            }
        }

        Err("UsbAlloc: Out of slots")
    }

    pub fn get_req_mut(&mut self, offset: usize) -> Result<&mut Request, &'static str> {
        self.bytes.as_type_mut(offset)
    }

    pub fn alloc_qh(&mut self, qh: QueueHead) -> Result<(usize, u32), &'static str> {
        for i in 0..self.qh_slots {
            let obj = self.qh_obj_offset + i;
            if !self.in_use[obj] {
                self.in_use[obj] = true;
                let offset = self.qh_offset + i * size_of::<QueueHead>();
                let mut_ref = self.bytes.as_type_mut(offset)?;
                *mut_ref = qh;
                let addr = mut_ref as *const QueueHead as usize;

                return Ok((offset, addr as u32))
            }
        }

        Err("UsbAlloc: Out of slots")
    }

    pub fn get_qh_mut(&mut self, offset: usize) -> Result<&mut QueueHead, &'static str> {
        self.bytes.as_type_mut(offset)
    }

    pub fn alloc_qtd(&mut self, qtd: TransferDescriptor) -> Result<(usize, u32), &'static str> {
        for i in 0..self.qtd_slots {
            let obj = self.qtd_obj_offset + i;
            if !self.in_use[obj] {
                self.in_use[obj] = true;
                let offset = self.qtd_offset + i * size_of::<TransferDescriptor>();
                let mut_ref = self.bytes.as_type_mut(offset)?;
                *mut_ref = qtd;
                let addr = mut_ref as *const TransferDescriptor as usize;

                return Ok((offset, addr as u32))
            }
        }

        Err("UsbAlloc: Out of slots")
    }

    pub fn get_qtd_mut(&mut self, offset: usize) -> Result<&mut TransferDescriptor, &'static str> {
        self.bytes.as_type_mut(offset)
    }

    pub fn alloc_desc(&mut self, desc: DeviceDescriptor) -> Result<(usize, u32), &'static str> {
        for i in 0..self.desc_slots {
            let obj = self.desc_obj_offset + i;
            if !self.in_use[obj] {
                self.in_use[obj] = true;
                let offset = self.desc_offset + i * size_of::<DeviceDescriptor>();
                let mut_ref = self.bytes.as_type_mut(offset)?;
                *mut_ref = desc;
                let addr = mut_ref as *const DeviceDescriptor as usize;

                return Ok((offset, addr as u32))
            }
        }

        Err("UsbAlloc: Out of slots")
    }

    pub fn get_desc_mut(&mut self, offset: usize) -> Result<&mut DeviceDescriptor, &'static str> {
        self.bytes.as_type_mut(offset)
    }
}


/// Memory-Mapped EHCI Capability Registers
#[derive(Debug, FromBytes)]
#[repr(C)]
struct CapabilityRegisters {
    cap_length: ReadOnly<u8>,
    _padding1: u8,
    hci_version: ReadOnly<u16>,
    hcs_params: ReadOnly<HcsParams>,
    hcc_params: ReadOnly<HccParams>,
    hcsp_port_route: ReadOnly<u64>,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct HcsParams {
    port_num: u4,
    port_power_control: bool,
    reserved: u2,
    supports_port_route: bool,
    ports_per_comp_ctrl: u4,
    comp_ctrl_num: u4,
    port_indicators: bool,
    reserved: u3,
    debug_port_num: u4,
    reserved: u8,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct HccParams {
    supports_64_bit_addressing: bool,
    can_set_frame_list_size: bool,
    supports_async_schedule_park: bool,
    reserved: bool,
    isochronous_scheduling_threshold: u4,
    ehci_ext_capp: u8,
    reserved: u16,
}

/// Memory-Mapped EHCI Operation Registers
#[derive(Debug, FromBytes)]
#[repr(C)]
struct OperationRegisters {
    command: Volatile<UsbCmd>,
    status: Volatile<UsbSts>,
    interrupts: Volatile<UsbIntr>,
    frame_index: Volatile<u32>,
    // holds the 32 MSBs of all addresses specified using
    // u32s in USB structures. See 2.3.5 of EHCI spec.
    segment: Volatile<u32>,
    periodic_list: Volatile<u32>,
    async_list: Volatile<u32>,
    _padding: [u32; 9],
    config_flag: Volatile<ConfigFlag>,
    // 16 is a maximum, there may actually be less slots
    ports: [Volatile<PortSc>; 16],
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct UsbCmd {
    run_stop: bool,
    host_ctrl_reset: bool,
    frame_list_size: FrameListSize,
    periodic_schedule: bool,
    async_schedule: bool,
    int_on_async_advance_doorbell: bool,
    light_host_ctrl_reset: bool,
    async_schedule_park_mode_count: u2,
    reserved: bool,
    async_schedule_park_mode: bool,
    reserved: u4,
    int_threshold_control: InterruptThreshold,
    reserved: u8,
}

// default is EightMicroFrames
#[bitsize(8)]
#[derive(Debug, FromBits)]
enum InterruptThreshold {
    OneMicroFrame        = 0x01,
    TwoMicroFrames       = 0x02,
    FourMicroFrames      = 0x04,
    EightMicroFrames     = 0x08,
    SixteenMicroFrames   = 0x10,
    ThirtyTwoMicroFrames = 0x20,
    SixtyFourMicroFrames = 0x40,
    #[fallback]
    Reserved = 0x7f,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum FrameListSize {
    /// 4096 bytes, 1024 dwords
    Full = 0,
    /// 2048 bytes, 512 dwords
    Half = 1,
    /// 1024 bytes, 256 dwords
    Quarter = 2,
    Reserved = 3,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
/// This register is generally read-only,
/// except for acknowledgements which
/// are done by writing a one to one of the
/// last six fields.
struct UsbSts {
    usb_int: bool,
    usb_error_int: bool,
    port_change_int: bool,
    frame_list_rollover_int: bool,
    host_system_error_int: bool,
    int_on_async_advance: bool,
    reserved: u6,
    hc_halted: bool,
    reclamation: bool,
    periodic_schedule_running: bool,
    async_schedule_running: bool,
    reserved: u16,
}

#[repr(u32)]
enum InterruptType {
    Usb               = 0x01,
    UsbError          = 0x02,
    PortChange        = 0x04,
    FrameListRollover = 0x08,
    HostSystemError   = 0x10,
    AsyncAdvance      = 0x20,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct UsbIntr {
    usb_int: bool,
    usb_error_int: bool,
    port_change_int: bool,
    frame_list_rollover_int: bool,
    host_system_error_int: bool,
    int_on_async_advance: bool,
    reserved: u26,
}

#[bitsize(1)]
#[derive(Debug, FromBits)]
enum ConfigureFlag {
    /// Port routing control logic default-routes each port to
    /// an implementation dependent classic host controller.
    Bypass = 0,
    /// Port routing control logic default-routes all ports to
    /// this host controller.
    Use = 1,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct ConfigFlag {
    inner: ConfigureFlag,
    reserved: u31,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct PortSc {
    connected: bool,
    connected_change: bool,
    // can only write false
    port_state: bool,
    port_state_change: bool,
    over_current: bool,
    over_current_change: bool,
    force_port_resume: bool,
    suspend: bool,
    port_reset: bool,
    reserved: bool,
    line_status: LineStatus,
    // if HSCPARAMS::PPC is set:
    // - bit is writable
    // - the host controller may clear this on overcurrent
    powered: bool,
    // true if this port is operated by a companion controller
    port_owner: bool,
    port_indicator: PortIndicatorControl,
    port_test: PortTestControl,
    wake_on_connect: bool,
    wake_on_disconnect: bool,
    wake_on_overcurrent: bool,
    reserved: u9,
}

#[bitsize(4)]
#[derive(Debug, FromBits)]
enum PortTestControl {
    Disabled = 0,
    TestJState = 1,
    TestKState = 2,
    TestSe0State = 3,
    TestPacket = 4,
    TestForceEnable = 5,
    #[fallback]
    Reserved = 0xf,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum PortIndicatorControl {
    Disabled = 0,
    Amber = 1,
    Green = 2,
    Undefined = 3,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum LineStatus {
    /// Not Low-speed device, perform EHCI reset
    Se0 = 0,
    /// Not Low-speed device, perform EHCI reset
    JState = 1,
    /// Low-speed device, release ownership of port
    KState = 2,
    /// Not Low-speed device, perform EHCI reset
    Undefined = 3,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct PointerNoTypeNoTerm {
    reserved: u5,
    addr_msbs: u27,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct PointerNoType {
    invalid: bool,
    reserved: u4,
    addr_msbs: u27,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct Pointer {
    invalid: bool,
    ptr_type: PointerType,
    reserved: u2,
    addr_msbs: u27,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum PointerType {
    IsochronousTransferDescriptor = 0,
    QueueHead = 1,
    SplitTransactionIsochronousTransferDescriptor = 2,
    FrameSpanTraversalNode = 3,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum HighBandwidthPipeMultiplier {
    Reserved = 0,
    One = 1,
    Two = 2,
    Three = 3,
}

#[derive(Debug, FromBytes)]
#[repr(C)]
struct IsochronousTransferDescriptor {
    next: Volatile<Pointer>,
    transactions: [Volatile<u32>; 8],
    bp0: Volatile<ItdBp0Register>,
    bp1: Volatile<ItdBp1Register>,
    bp2: Volatile<ItdBp2Register>,
    bp3: Volatile<ItdBpRegister>,
    bp4: Volatile<ItdBpRegister>,
    bp5: Volatile<ItdBpRegister>,
    bp6: Volatile<ItdBpRegister>,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct ItdTransactionRegister {
    buf_offset: u12,
    page_select: u3,
    int_on_complete: bool,
    buf_length: u12,
    // actual max: 0xC00
    transaction_error: bool,
    babble_detected: bool,
    buffer_error: bool,
    active: bool,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct ItdBp0Register {
    dev_addr: u7,
    reserved: bool,
    endpoint: u4,
    _ptr: u20,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct ItdBp1Register {
    max_packet_size: u11,
    direction: Direction,
    _ptr: u20,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct ItdBp2Register {
    multi: HighBandwidthPipeMultiplier,
    reserved: u10,
    _ptr: u20,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct ItdBpRegister {
    reserved: u12,
    _ptr: u20,
}

#[derive(Debug, FromBytes)]
#[repr(C)]
struct SplitTransactionIsochronousTransferDescriptor {
    next: Volatile<Pointer>,
    /// Endpoint and Transaction Translator Characteristics
    endpoint: Volatile<SiTdEndpoint>,
    /// Micro-frame Schedule Control
    micro_frame: Volatile<SiTdMicroFrame>,
    /// siTD Transfer Status and Control
    state: Volatile<SiTdState>,
    /// siTD Buffer Pointer 0
    bp0: Volatile<BufferPointerWithOffset>,
    /// siTD Buffer Pointer 1
    bp1: Volatile<SiTdBp1>,
    /// siTD Back Link Pointer
    blp: Volatile<PointerNoType>,
}

/// Endpoint and Transaction Translator Characteristics
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct SiTdEndpoint {
    dev_addr: u7,
    reserved: bool,
    endpoint: u4,
    reserved: u4,
    hub_addr: u7,
    reserved: bool,
    port_num: u7,
    direction: Direction,
}

/// Micro-frame Schedule Control
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct SiTdMicroFrame {
    split_start_mask: u8,
    split_completion_mask: u8,
    reserved: u16,
}

/// siTD Transfer Status and Control
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct SiTdState {
    reserved: bool,
    split_x_state: bool,
    missed_micro_frame: bool,
    transaction_error: bool,
    babble_detected: bool,
    data_buffer_error: bool,
    err_received: bool,
    active: bool,
    micro_frame_complete_split_progress_mask: u8,
    total_bytes_to_transfer: u10,
    reserved: u4,
    page_select: bool,
    int_on_complete: bool,
}

/// siTD Buffer Pointer 0
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct BufferPointerWithOffset {
    current_offset: u12,
    _ptr: u20,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct BufferPointer {
    reserved: u12,
    _ptr: u20,
}

/// siTD Buffer Pointer 1
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct SiTdBp1 {
    transaction_count: u3,
    transaction_pos: TransactionPosition,
    reserved: u7,
    _ptr: u20,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum TransactionPosition {
    All = 0,
    Begin = 1,
    Mid = 2,
    End = 3,
}

#[derive(Debug, FromBytes)]
#[repr(C)]
struct TransferDescriptor {
    next: Volatile<PointerNoType>,
    alt_next: Volatile<PointerNoType>,
    token: Volatile<QtdToken>,
    bp0: Volatile<BufferPointerWithOffset>,
    bp1: Volatile<BufferPointer>,
    bp2: Volatile<BufferPointer>,
    bp3: Volatile<BufferPointer>,
    bp4: Volatile<BufferPointer>,
}

/// siTD Buffer Pointer 1
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct QtdToken {
    ping: bool,
    split_x_state: bool,
    missed_micro_frame: bool,
    transaction_error: bool,
    babble_detected: bool,
    data_buffer_error: bool,
    halted: bool,
    active: bool,
    pid: PidCode,
    error_count: u2,
    current_page: u3,
    int_on_complete: bool,
    total_bytes_to_transfer: u15,
    data_toggle: bool,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum PidCode {
    Out = 0,
    In = 1,
    Setup = 2,
    Reserved = 3,
}

#[derive(Debug, FromBytes)]
#[repr(C)]
struct QueueHead {
    next: Volatile<Pointer>,
    /// Endpoint Capabilities and Characteristics
    reg0: Volatile<QhEndpoint>,
    /// Micro-frame Schedule Control
    reg1: Volatile<QhMicroFrame>,
    /// Current Qtd Pointer
    current_qtd: Volatile<PointerNoTypeNoTerm>,

    // Transfer Overlay
    next_qtd: Volatile<PointerNoType>,
    alt_next_qtd: Volatile<AltNextQtdPointer>,
    token: Volatile<QtdToken>,
    bp0: Volatile<BufferPointerWithOffset>,
    bp1: Volatile<QhBp1>,
    bp2: Volatile<QhBp2>,
    bp3: Volatile<BufferPointer>,
    bp4: Volatile<BufferPointer>,
}

/// Alternate Next qTD Pointer
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct AltNextQtdPointer {
    valid: bool,
    nak_counter: u4,
    _ptr: u27,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct QhBp1 {
    split_transaction_complete_split_progress: u8,
    reserved: u4,
    _ptr: u20,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct QhBp2 {
    split_transaction_frame_tag: u5,
    s_bytes: u7,
    _ptr: u20,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct QhEndpoint {
    device: u7,
    // warning: complex
    inactivate_on_next_transaction: bool,
    endpoint: u4,
    endpoint_speed: EndpointSpeed,
    import_data_toggle_from_qtd: bool,
    head_of_reclamation_list: bool,
    // max value: 0x400
    max_packet_len: u11,
    control_endpoint: bool,
    nak_count_reload: u4,
}

#[bitsize(2)]
#[derive(Debug, FromBits)]
enum EndpointSpeed {
    // 12Mbps
    FullSpeed = 0,
    // 1.5Mbps
    LowSpeed = 1,
    // 480Mbps
    HighSpeed = 2,
    Reserved = 3,
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct QhMicroFrame {
    int_schedule_mask: u8,
    split_completion_mask: u8,
    hub_addr: u7,
    port_num: u7,
    high_bandwidth_pipe_mul: HighBandwidthPipeMultiplier,
}
