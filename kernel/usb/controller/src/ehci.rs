#![allow(dead_code)]

use super::*;

allocator!(TransferDescriptorAlloc, TransferDescriptor, 128);
allocator!(QueueHeadAlloc, QueueHead, 32);

fn sleep_ms(milliseconds: u64) {
    sleep(Duration::from_millis(milliseconds))
        .expect("[USB-EHCI] Failed to sleep for 10ms");
}

macro_rules! try_wait_until {
    ($poll_interval_ms:literal, $timeout_ms:literal, $cond:expr) => {{
        let mut elapsed = 0;

        while !($cond) && elapsed < $timeout_ms {
            sleep_ms($poll_interval_ms);
            elapsed += $poll_interval_ms;
        }


        if elapsed >= $timeout_ms {
            log::error!("[USB-EHCI] line {}: Timeout expiry", line!());
            Err("[USB-EHCI] Timeout expiry")
        } else {
            Ok(())
        }
    }}
}

#[derive(Debug, FromBytes)]
pub struct UsbAlloc {
    common: CommonUsbAlloc,
    transfer_descriptors: TransferDescriptorAlloc,
    queue_heads: QueueHeadAlloc,
}

#[derive(Debug)]
pub struct EhciController {
    devices: u128,
    config_space: MappedPages,
    hcs_params: HcsParams,
    op_offset: usize,
    usb_alloc: MappedPages,
}

impl EhciController {
    pub fn new(ehci_pci_dev: &PciDevice) -> Result<Self, &'static str> {
        let base = (ehci_pci_dev.bars[0] as usize) & !0xff;
        let base = PhysicalAddress::new(base).ok_or("Invalid PCI BAR for EHCI USB controller")?;

        let (mut usb_alloc, four_gig_segment) = {
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

        // this installs a single dummy queue head in the asynchronous schedule,
        // which makes asynchronous queue management easier.
        op_regs.async_list.write({
            let usb_alloc = usb_alloc.as_type_mut::<UsbAlloc>(0)?;

            let first_qtd_none = PointerNoType::from(1);
            let dummy_queue_head = create_queue_head(true, 0, 0, first_qtd_none);
            let (index, dqh_addr) = usb_alloc.queue_heads.allocate(Some(dummy_queue_head))?;

            // close the loop
            let qdh_mut = usb_alloc.queue_heads.get_mut(index)?;
            qdh_mut.next.write(queue_head_pointer(dqh_addr));

            dqh_addr
        });

        sleep(Duration::from_millis(10)).unwrap();

        log::info!("Initialized an EHCI USB controller with {} ports and {} companion controllers",
            hcs_params.port_num(),
            hcs_params.comp_ctrl_num());

        Ok(Self {
            devices: 0,
            config_space,
            hcs_params,
            op_offset,
            usb_alloc,
        })
    }

    fn alloc(&self) -> Result<&UsbAlloc, &'static str> {
        self.usb_alloc.as_type::<UsbAlloc>(0)
    }

    fn op_regs(&self) -> Result<&OperationRegisters, &'static str> {
        self.config_space.as_type::<OperationRegisters>(self.op_offset)
    }

    fn alloc_mut(&mut self) -> Result<&mut UsbAlloc, &'static str> {
        self.usb_alloc.as_type_mut::<UsbAlloc>(0)
    }

    fn op_regs_mut(&mut self) -> Result<&mut OperationRegisters, &'static str> {
        self.config_space.as_type_mut::<OperationRegisters>(self.op_offset)
    }

    pub fn probe_ports(&mut self) -> Result<(), &'static str> {
        let port_num = self.hcs_params.port_num().value() as usize;
        for i in 0..port_num {
            let port = &mut self.op_regs_mut()?.ports[i];
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

                    let mut addr = Err("Out of device addresses");

                    for i in 0..128 {
                        let mask = 1 << i;
                        if self.devices & mask == 0 {
                            self.devices |= mask;
                            addr = Ok(i);
                            break;
                        }
                    }

                    let addr = addr?;

                    self.request(0, Request::SetAddress(addr), 8)?;

                    let mut device = descriptors::Device::default();
                    self.request(addr, Request::GetDeviceDescriptor(&mut device), 8)?;
                    log::warn!("device_descriptor: {:#x?}", device);

                    let max_packet_size = device.max_packet_size;

                    let mut config = unsafe { core::mem::MaybeUninit::<descriptors::Configuration>::zeroed().assume_init() };
                    self.request(addr, Request::GetConfigDescriptor(0, &mut config), max_packet_size)?;
                    log::warn!("config 0: {:#x?}", config.inner);

                    let mut offset = 0;
                    for i in 0..config.inner.num_interfaces {
                        let (interface, o): (&descriptors::Interface, _) = config.find_desc(offset, DescriptorType::Interface)?;
                        log::warn!("interface: {}", interface.name);
                        for _ in 0..interface.num_endpoints {
                            // todo: read HID descriptor here
                            let (endpoint, o): (&descriptors::Endpoint, _) = config.find_desc(offset, DescriptorType::Endpoint)?;
                            log::warn!("endpoint: {:#x?}", endpoint);
                            offset = o;
                        }

                        if interface.class == 3 {
                            self.request(addr, Request::HidSetProtocol(i as _, request::HidProtocol::Boot), max_packet_size)?;

                            loop {
                                let report_type = request::HidReportType::Input;
                                let report_id = 0;
                                let mut report = [0u8; 8];
                                self.request(addr, Request::HidGetReport(i as _, report_type, report_id, &mut report), max_packet_size)?;

                                log::warn!("report: {:x?}", report);
                                sleep_ms(50);
                            }

                            /*
                            // set leds
                            let report_type = request::HidReportType::Output;
                            let report_id = 0;
                            let report = [0b00011111];
                            self.request(addr, Request::HidSetReport(i as _, report_type, report_id, &report), max_packet_size)?;
                            */
                        }

                        offset = o;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn turn_off(&mut self) -> Result<(), &'static str> {
        self.op_regs_mut()?.command.update(|cmd| cmd.set_run_stop(false));
        Ok(())
    }

    pub fn request(&mut self, dev_addr: DeviceAddress, request: Request, max_packet_size: u8) -> Result<(), &'static str> {
        let mut raw_req = request.get_raw();

        let shmem = self.alloc_mut()?;
        let (shmem_index, shmem_addr) = request.allocate_payload(&mut shmem.common)?;

        let mut first_pass = true;
        loop {
            let shmem = self.alloc_mut()?;
            // todo: handle device descriptors case (smaller data size than needed)
            let data_sz = raw_req.len() as usize;

            let (qh_addr, first_qtd_index, req_index) = create_request(shmem, dev_addr, raw_req, data_sz, max_packet_size, shmem_addr)?;

            self.push_to_async_schedule(qh_addr)?;
            self.wait_for_all_td_inactive(first_qtd_index)?;
            self.remove_from_async_schedule(qh_addr)?;
            self.free_qh_and_qtd(qh_addr, first_qtd_index)?;

            let shmem = self.alloc_mut()?;
            shmem.common.requests.free(req_index)?;

            if first_pass {
                match request.adjust_len(&shmem.common, shmem_index)? {
                    Some(length_update) => raw_req.set_len(length_update),
                    None => break,
                }

                first_pass = false;
            } else {
                break;
            }
        }

        let shmem = self.alloc_mut()?;
        request.free_and_move_payload(&mut shmem.common, shmem_index)
    }

    fn free_qh_and_qtd(&mut self, qh_addr: u32, first_qtd_index: usize) -> Result<(), &'static str> {
        let shmem = self.alloc_mut()?;

        let qh_index = shmem.queue_heads.find(qh_addr)?;
        shmem.queue_heads.free(qh_index)?;

        let mut qtd_index = first_qtd_index;
        let no_next = PointerNoType::from(1);

        loop {
            let qtd_ref = shmem.transfer_descriptors.free(qtd_index)?;
            let next_pointer = qtd_ref.next.read();
            if next_pointer == no_next {
                return Ok(());
            } else {
                let next_addr = next_pointer.address();
                qtd_index = shmem.transfer_descriptors.find(next_addr)?;
            }
        }
    }

    fn wait_for_all_td_inactive(&self, first_qtd_index: usize) -> Result<(), &'static str> {
        let transfer_descriptors = &self.alloc()?.transfer_descriptors;
        let mut qtd_index = first_qtd_index;
        let no_next = PointerNoType::from(1);

        loop {
            let qtd_ref = transfer_descriptors.get(qtd_index)?;
            try_wait_until!(1, 1000, !qtd_ref.token.read().active())?;

            let next_pointer = qtd_ref.next.read();
            if next_pointer == no_next {
                return Ok(());
            } else {
                let next_addr = next_pointer.address();
                qtd_index = transfer_descriptors.find(next_addr)?;
            }
        }
    }

    fn enable_async_schedule(&mut self, enable: bool) -> Result<(), &'static str> {
        let op_regs = self.op_regs_mut()?;
        op_regs.command.update(|cmd| cmd.set_async_schedule(enable));
        try_wait_until!(1, 1000, op_regs.status.read().async_schedule_running() == enable)?;
        Ok(())
    }

    fn get_async_schedule_prev(&self, queue_head_addr: u32) -> Result<usize, &'static str> {
        let queue_heads = &self.alloc()?.queue_heads;
        let mut current_index = queue_heads.find(queue_head_addr)?;

        loop {
            let next_pointer = queue_heads.get(current_index)?.next.read();
            let next_addr = next_pointer.address();
            if next_addr == queue_head_addr {
                return Ok(current_index);
            } else {
                current_index = queue_heads.find(next_addr)?;
            }
        }
    }

    fn push_to_async_schedule(&mut self, to_push_addr: u32) -> Result<(), &'static str> {
        self.enable_async_schedule(false)?;

        let first_addr = self.op_regs()?.async_list.read();
        let last_index = self.get_async_schedule_prev(first_addr)?;

        let queue_heads = &mut self.alloc_mut()?.queue_heads;

        // set `to_push` as next of `last`
        let last_mut = queue_heads.get_mut(last_index)?;
        last_mut.next.write(queue_head_pointer(to_push_addr));

        // set next of `to_push` to `first`
        let to_push_mut = queue_heads.get_mut_by_addr(to_push_addr)?;
        to_push_mut.next.write(queue_head_pointer(first_addr));

        self.enable_async_schedule(true)
    }

    fn remove_from_async_schedule(&mut self, to_remove_addr: u32) -> Result<(), &'static str> {
        self.enable_async_schedule(false)?;

        // I'm not sure if the controller can actually advance the pointer in ASYNCLISTADDR.
        // If it can, this will probably fail easily.
        let first_addr = self.op_regs()?.async_list.read();
        assert_ne!(first_addr, to_remove_addr, "[USB-EHCI] Tried to remove a queue head while the controller was using it.");

        let prev_index = self.get_async_schedule_prev(to_remove_addr)?;
        let queue_heads = &mut self.alloc_mut()?.queue_heads;

        // read next of `to_remove`
        let to_remove_ref = queue_heads.get_by_addr(to_remove_addr)?;
        let next_pointer = to_remove_ref.next.read();

        // skip `to_remove`
        let prev_mut = queue_heads.get_mut(prev_index)?;
        prev_mut.next.write(next_pointer);

        let prev_addr = queue_heads.address_of(prev_index)?;
        let next_addr = next_pointer.address();
        if prev_addr != next_addr {
            self.enable_async_schedule(true)
        } else {
            // if the queue has only one element, it has to be the default/dummy queue head.
            // this queue element is a dummy one, so there's no point in re-enabling the async schedule.
            Ok(())
        }
    }
}

fn create_request(
    shmem: &mut UsbAlloc,
    dev_addr: DeviceAddress,
    req: RawRequest,
    mut data_size: usize,
    max_packet_size: u8,
    mut in_buf: u32,
) -> Result<(u32, usize, usize), &'static str> {
    let max_packet_size = max_packet_size as usize;
    let (req_index, req_ptr) = shmem.common.requests.allocate(Some(req))?;
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
        u15::new(size_of::<RawRequest>() as _),
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

    let (first_qtd_index, first_qtd_ptr) = shmem.transfer_descriptors.allocate(Some(setup_qtd))?;
    let mut prev_qtd_index = first_qtd_index;

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

        let (part_qtd_index, part_qtd_ptr) = shmem.transfer_descriptors.allocate(Some(in_qtd))?;

        let prev_qtd = shmem.transfer_descriptors.get_mut(prev_qtd_index)?;
        prev_qtd.next.write(PointerNoType::from(part_qtd_ptr));

        prev_qtd_index = part_qtd_index;

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

    let (_status_qtd_index, status_qtd_ptr) = shmem.transfer_descriptors.allocate(Some(status_qtd))?;

    let prev_qtd = shmem.transfer_descriptors.get_mut(prev_qtd_index)?;
    prev_qtd.next.write(PointerNoType::from(status_qtd_ptr));

    let first_qtd_bp = PointerNoType::from(first_qtd_ptr);
    let qh = create_queue_head(false, dev_addr, max_packet_size, first_qtd_bp);

    let (_qh_index, queue_head_addr) = shmem.queue_heads.allocate(Some(qh))?;

    Ok((queue_head_addr, first_qtd_index, req_index))
}

fn create_queue_head(is_first_qh: bool, dev_addr: u8, max_packet_size: usize, first_qtd_bp: PointerNoType) -> QueueHead {
    let qh_endpoint = QhEndpoint::new(
        u7::new(dev_addr),
        false,
        u4::new(0),
        EndpointSpeed::HighSpeed,
        true,
        is_first_qh,
        u11::new(max_packet_size as _),
        true,
        u4::new(0),
    );

    let qh_uframe = QhMicroFrame::new(
        0, 0, u7::new(0), u7::new(0),
        HighBandwidthPipeMultiplier::One,
    );

    let to_be_changed_at_insertion = Pointer::from(1);
    let alt_no_next = AltNextQtdPointer::from(1);

    QueueHead {
        next: Volatile::new(to_be_changed_at_insertion),
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
    }
}

fn queue_head_pointer(queue_head_addr: u32) -> Pointer {
    Pointer::new(false, PointerType::QueueHead, u27::new(queue_head_addr >> 5))
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

/// This register is generally read-only,
/// except for acknowledgements which
/// are done by writing a one to one of the
/// last six fields.
#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
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
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes, PartialEq)]
struct PointerNoType {
    invalid: bool,
    reserved: u4,
    addr_msbs: u27,
}

impl PointerNoType {
    fn address(&self) -> u32 {
        self.addr_msbs().value() << 5
    }
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct Pointer {
    invalid: bool,
    ptr_type: PointerType,
    reserved: u2,
    addr_msbs: u27,
}

impl Pointer {
    fn address(&self) -> u32 {
        self.addr_msbs().value() << 5
    }
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
#[derive(Debug, FromBytes, Clone)]
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

#[derive(Debug, FromBytes, Clone)]
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
