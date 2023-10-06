#![allow(dead_code)]

use hashbrown::HashMap;

use super::*;

type DeviceAddress = u8;

#[derive(Debug)]
struct Device {
    descriptor: DeviceDescriptor,
}

allocator!(DeviceDescriptorAlloc, DeviceDescriptor, 16);
allocator!(TransferDescriptorAlloc, TransferDescriptor, 16);
allocator!(QueueHeadAlloc, QueueHead, 16);
allocator!(RequestAlloc, Request, 16);

impl Device {
    pub fn new(ctrl: &mut EhciController) -> Result<Self, &'static str> {
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

        let shmem = ctrl.alloc()?;
        let (desc_offset, desc) = shmem.device_descriptors.alloc(DeviceDescriptor::default())?;
        let (qh, _qh_index, setup, status) = create_request(shmem, 0, req, data_sz, 8, desc)?;

        let op_regs = ctrl.op_regs()?;
        op_regs.async_list.write(qh);
        op_regs.command.update(|cmd| cmd.set_async_schedule(true));

        let mut timeout = 10;

        let shmem = ctrl.alloc()?;
        loop {
            let setup_qtd = shmem.queue_heads.get(setup)?;
            if timeout == 0 {
                return Err("Timeout 1");
            } else if setup_qtd.token.read().active() {
                timeout -= 1;
                sleep(Duration::from_millis(10)).unwrap();
            } else {
                break;
            }
        }

        timeout = 10;

        loop {
            let setup_qtd = shmem.transfer_descriptors.get(status)?;
            if timeout == 0 {
                return Err("Timeout 2");
            } else if setup_qtd.token.read().active() {
                timeout -= 1;
                sleep(Duration::from_millis(10)).unwrap();
            } else {
                break;
            }
        }

        let op_regs = ctrl.op_regs()?;
        op_regs.command.update(|cmd| cmd.set_async_schedule(false));

        let shmem = ctrl.alloc()?;
        let descriptor = *shmem.device_descriptors.get(desc_offset)?;
        log::warn!("DESCRIPTOR: {:#x?}", descriptor);

        Ok(Self {
            descriptor,
        })
    }

    pub fn init(&mut self, _address: DeviceAddress) {
        // todo
    }
}

#[derive(Debug, FromBytes)]
pub struct UsbAlloc {
    device_descriptors: DeviceDescriptorAlloc,
    transfer_descriptors: TransferDescriptorAlloc,
    queue_heads: QueueHeadAlloc,
    requests: RequestAlloc,
}

#[derive(Debug)]
pub struct EhciController {
    devices: HashMap<DeviceAddress, Device>,
    config_space: MappedPages,
    hcs_params: HcsParams,
    op_offset: usize,
    usb_alloc: MappedPages,
}

impl EhciController {
    pub fn new(ehci_pci_dev: &PciDevice) -> Result<Self, &'static str> {
        let base = (ehci_pci_dev.bars[0] as usize) & !0xff;
        let base = PhysicalAddress::new(base).ok_or("Invalid PCI BAR for EHCI USB controller")?;

        let (usb_alloc, four_gig_segment) = {
            // todo 1: make sure this doesn't cross a 4GiB boundary
            // todo 2: no need to ID-map this but then
            // I'd need to convert pages to frames very often
            let needed_mem = size_of::<UsbAlloc>();
            log::info!("EHCI USB allocator size: {} bytes", needed_mem);
            let num_pages = (needed_mem + (PAGE_SIZE - 1)) / PAGE_SIZE;
            let mut usb_alloc = create_identity_mapping(num_pages, MMIO_FLAGS)?;
            usb_alloc.as_slice_mut(0, needed_mem)?.fill(0u8);
            let addr = usb_alloc.start_address().value();
            let four_gig_segment = (addr >> 32) as u32;
            log::info!("EHCI USB allocator virtual addr: 0x{:x}", addr);
            (usb_alloc, four_gig_segment)
        };

        let mut config_space = map_frame_range(base, PAGE_SIZE, MMIO_FLAGS)?;
        let capa_regs = config_space.as_type::<CapabilityRegisters>(0)?;

        let op_offset = capa_regs.cap_length.read() as usize;
        let hcs_params = capa_regs.hcs_params.read();

        let op_regs = config_space.as_type_mut::<OperationRegisters>(op_offset)?;

        op_regs.segment.write(four_gig_segment);
        op_regs.command.update(|cmd| cmd.set_async_schedule(false));
        op_regs.command.update(|cmd| cmd.set_periodic_schedule(false));
        op_regs.command.update(|cmd| cmd.set_run_stop(true));
        op_regs.config_flag.update(|cmd| cmd.set_inner(ConfigureFlag::Use));

        sleep(Duration::from_millis(10)).unwrap();

        log::info!("Initialized an EHCI USB controller with {} ports and {} companion controllers",
            hcs_params.port_num(),
            hcs_params.comp_ctrl_num());

        Ok(Self {
            devices: HashMap::new(),
            config_space,
            hcs_params,
            op_offset,
            usb_alloc,
        })
    }

    fn alloc(&mut self) -> Result<&mut UsbAlloc, &'static str> {
        self.usb_alloc.as_type_mut::<UsbAlloc>(0)
    }

    fn op_regs(&mut self) -> Result<&mut OperationRegisters, &'static str> {
        self.config_space.as_type_mut::<OperationRegisters>(self.op_offset)
    }

    pub fn probe_ports(&mut self) -> Result<(), &'static str> {
        let port_num = self.hcs_params.port_num().value() as usize;
        for i in 0..port_num {
            let port = &mut self.op_regs()?.ports[i];
            log::error!("P{}.connected_change: {}", i, port.read().connected_change());
            if port.read().connected_change() {
                // writing true makes it false (spec)
                port.update(|port| port.set_connected_change(true));

                if port.read().connected() {
                    // reset the device; it will now reply to requests targeted at address zero
                    port.update(|port| port.set_port_state(false));
                    port.update(|port| port.set_port_reset(true));
                    sleep(Duration::from_millis(10)).unwrap();
                    port.update(|port| port.set_port_reset(false));
                    log::error!("CONNECTED: {:#?}", port.read());

                    let mut device = Device::new(self)?;
                    let mut addr = Err("Out of device addresses");

                    for i in 0..128 {
                        if !self.devices.contains_key(&i) {
                            addr = Ok(i);
                            break;
                        }
                    }

                    device.init(addr?);
                }
            }
        }

        Ok(())
    }

    pub fn turn_off(&mut self) -> Result<(), &'static str> {
        self.op_regs()?.command.update(|cmd| cmd.set_run_stop(false));
        Ok(())
    }
}

fn create_request(
    shmem: &mut UsbAlloc,
    dev_addr: u8,
    req: Request,
    mut data_size: usize,
    max_packet_size: usize,
    mut in_buf: u32,
) -> Result<(u32, usize, usize, usize), &'static str> {
    let (_, req_ptr) = shmem.requests.alloc(req)?;
    let req_bp = BufferPointerWithOffset::from(req_ptr);

    let zero_bp = BufferPointer::from(0);
    let no_next = PointerNoType::from(1);
    let alt_no_next = AltNextQtdPointer::from(1);
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

    let (first_qtd_offset, first_qtd_ptr) = shmem.transfer_descriptors.alloc(setup_qtd)?;
    let mut prev_qtd_offset = first_qtd_offset;

    let pid_code = match req.direction() {
        Direction::Out => PidCode::Out,
        Direction::In => PidCode::In,
    };

    while data_size > 0 {
        let progress = data_size.min(max_packet_size);

        let in_token = QtdToken::new(
            // initial status flags:
            false, false, false, false, false, false, false,

            true,
            pid_code,
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

        let (part_qtd_offset, part_qtd_ptr) = shmem.transfer_descriptors.alloc(in_qtd)?;

        let prev_qtd = shmem.transfer_descriptors.get_mut(prev_qtd_offset)?;
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

    let (status_qtd_offset, status_qtd_ptr) = shmem.transfer_descriptors.alloc(status_qtd)?;

    let prev_qtd = shmem.transfer_descriptors.get_mut(prev_qtd_offset)?;
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
        alt_next_qtd: Volatile::new(alt_no_next),
        token: Volatile::new(QtdToken::from(0)),
        bp0: Volatile::new(BufferPointerWithOffset::from(0)),
        bp1: Volatile::new(QhBp1::from(0)),
        bp2: Volatile::new(QhBp2::from(0)),
        bp3: Volatile::new(BufferPointer::from(0)),
        bp4: Volatile::new(BufferPointer::from(0)),
    };

    let (qh_offset, queue_head) = shmem.queue_heads.alloc(qh)?;

    // circular queue
    let qh_mut = shmem.queue_heads.get_mut(qh_offset)?;
    qh_mut.next.write(Pointer::new(false, PointerType::QueueHead, u27::new(queue_head >> 5)));

    Ok((queue_head, qh_offset, first_qtd_offset, status_qtd_offset))
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

/// Queue Element Transfer Descriptor
#[derive(Debug, FromBytes)]
#[repr(C, align(32))]
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
#[derive(Copy, Clone, Debug, FromBits)]
enum PidCode {
    Out = 0,
    In = 1,
    Setup = 2,
    Reserved = 3,
}

#[derive(Debug, FromBytes)]
#[repr(C, align(32))]
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
