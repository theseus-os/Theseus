/// EHCI Controller support

use super::*;

use interfaces::InterruptTransferAction;

allocator!(TransferDescriptorAlloc, TransferDescriptor, 128);
allocator!(QueueHeadAlloc, QueueHead, 32);

const PERIODIC_LIST_LEN: usize = 1024;

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
    periodic_list: PeriodicList,
}

pub struct EhciController {
    // Nth bit set = device address N taken; address 0 is invalid
    devices: u128,
    hcs_params: HcsParams,
    op_regs: BorrowedMappedPages<OperationRegisters, Mutable>,
    usb_alloc: BorrowedMappedPages<UsbAlloc, Mutable>,
    pending_int_transfers: Vec<InterruptTransfer>,
    interfaces: Vec<Option<interfaces::Interface>>,
    initialized: bool,
}

#[derive(Debug, Copy, Clone)]
struct InterruptTransfer {
    interface_id: InterfaceId,
    endpoint: EndpointAddress,
    ep_max_packet_size: MaxPacketSize,
    queue_head: UsbPointer,
    first_qtd_index: AllocSlot,
    buffer: (UsbPointer, usize),
}

impl Drop for EhciController {
    fn drop(&mut self) {
        self.turn_off().unwrap();
    }
}

impl ControllerApi for EhciController {
    fn common_alloc_mut(&mut self) -> Result<&mut CommonUsbAlloc, &'static str> {
        Ok(&mut self.usb_alloc.common)
    }

    fn probe_ports(&mut self) -> Result<(), &'static str> {
        let port_num = self.hcs_params.port_num().value() as usize;

        for i in 0..port_num {
            let port = &mut self.op_regs.ports[i];
            log::error!("P{}.connected_change: {}", i, port.read().connected_change());
            if port.read().connected_change() || !self.initialized {
                // writing true makes it false (spec)
                port.update(|port| port.set_connected_change(true));

                if port.read().connected() {
                    // reset the device; it will now reply to requests targeted at address zero
                    port.update(|port| port.set_port_state(false));
                    port.update(|port| port.set_port_reset(true));
                    sleep_ms(10);
                    port.update(|port| port.set_port_reset(false));
                    log::info!("CONNECTED PORT: {}", i);

                    let mut addr = Err("Out of device addresses");

                    for i in 1..128 {
                        let mask = 1 << i;
                        if self.devices & mask == 0 {
                            self.devices |= mask;
                            addr = Ok(i);
                            break;
                        }
                    }

                    let addr = addr?;
                    log::info!("Assigning address: {}", addr);

                    self.request(0, Request::SetAddress(addr), 8)?;
                    log::info!("Assigned address: {}", addr);

                    self.init_device(addr)?;
                } else {
                    // todo: handle disconnection
                }
            }
        }

        self.initialized = true;

        Ok(())
    }

    fn request(
        &mut self,
        dev_addr: DeviceAddress,
        request: Request,
        max_packet_size: MaxPacketSize,
    ) -> Result<(), &'static str> {
        let mut raw_req = request.get_raw();
        let (shmem_index, shmem_addr) = request.allocate_payload(&mut self.usb_alloc.common)?;

        let mut first_pass = true;
        loop {
            // todo: handle device descriptors case (smaller data size than needed)
            let data_sz = raw_req.len() as usize;

            let (qh_addr, first_qtd_index, req_index) = create_request(
                &mut self.usb_alloc,
                dev_addr,
                raw_req,
                data_sz,
                max_packet_size,
                shmem_addr,
            )?;

            self.push_to_async_schedule(qh_addr)?;
            self.wait_for_all_td_inactive(first_qtd_index)?;
            self.remove_from_async_schedule(qh_addr)?;
            self.qtd_error_check(first_qtd_index)?;
            self.qh_and_qtd_cleanup(Some(qh_addr), first_qtd_index)?;

            self.usb_alloc.common.requests.free(req_index)?;

            if first_pass {
                match request.adjust_len(&self.usb_alloc.common, shmem_index)? {
                    Some(length_update) => raw_req.set_len(length_update),
                    None => break,
                }

                first_pass = false;
            } else {
                break;
            }
        }

        let shmem_common = &mut self.usb_alloc.common;
        request.free_and_move_payload(shmem_common, shmem_index)
    }

    fn setup_interrupt_transfer(
        &mut self,
        device_addr: DeviceAddress,
        interface_id: InterfaceId,
        endpoint: EndpointAddress,
        ep_max_packet_size: MaxPacketSize,
        buffer: UsbPointer,
        size: usize,
    ) -> Result<(), &'static str> {
        // TODO: handle polling interval

        let (queue_head, first_qtd_index) = create_int_transfer_qh(
            &mut self.usb_alloc,
            device_addr,
            endpoint,
            ep_max_packet_size,
            buffer,
            size,
        )?;

        let new_transfer = InterruptTransfer {
            interface_id,
            endpoint,
            ep_max_packet_size,
            queue_head,
            first_qtd_index,
            buffer: (buffer, size),
        };

        self.pending_int_transfers.push(new_transfer);
        self.enable_periodic_schedule(false)?;
        self.populate_periodic_schedule()
    }

    fn handle_interrupt(&mut self) -> Result<(), &'static str> {
        let status = self.op_regs.status.read();

        if status.usb_int() {
            self.op_regs.status.update(|sts| sts.set_usb_int(true));

            let mut i = 0;
            while let Some(transfer) = self.pending_int_transfers.get(i) {
                let queue_head = self.usb_alloc.queue_heads.get_mut_by_addr(transfer.queue_head)?;
                if !queue_head.token.read().active() {
                    let dev_addr = u8::from(queue_head.reg0.read().device());
                    if self.handle_interrupt_transfer(dev_addr, i)? {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }

        if status.usb_error_int() {
            self.op_regs.status.update(|sts| sts.set_usb_error_int(true));
            log::error!("TODO: USB-EHCI interrupt: USB Error");
        }

        if status.port_change_int() {
            self.op_regs.status.update(|sts| sts.set_port_change_int(true));
            log::error!("TODO: USB-EHCI interrupt: Port Change");
        }

        if status.frame_list_rollover_int() {
            self.op_regs.status.update(|sts| sts.set_frame_list_rollover_int(true));
            log::error!("TODO: USB-EHCI interrupt: Frame List Rollover");
        }

        if status.host_system_error_int() {
            self.op_regs.status.update(|sts| sts.set_host_system_error_int(true));
            log::error!("TODO: USB-EHCI interrupt: Host System Error");
        }

        if status.int_on_async_advance() {
            self.op_regs.status.update(|sts| sts.set_int_on_async_advance(true));
            log::error!("TODO: USB-EHCI interrupt: Async Advance");
        }

        log::warn!("Done handling an USB interrupt\n{:#?}", self.op_regs.status.read());

        Ok(())
    }
}

impl EhciController {
    pub fn init(base: PhysicalAddress) -> Result<Self, &'static str> {
        let (mut usb_alloc, four_gig_segment) = {
            // todo 1: make sure this doesn't cross a 4GiB boundary
            // todo 2: no need to ID-map this but then
            // I'd need to convert pages to frames very often
            let needed_mem = size_of::<UsbAlloc>();
            let num_pages = (needed_mem + (PAGE_SIZE - 1)) / PAGE_SIZE;
            log::info!("EHCI USB allocator size: {} bytes", needed_mem);

            let mut usb_alloc = create_identity_mapping(num_pages, MMIO_FLAGS)?;

            // zero all structs
            usb_alloc.as_slice_mut(0, needed_mem)?.fill(0u8);

            let addr = usb_alloc.start_address().value();
            let four_gig_segment = (addr >> 32) as u32;
            log::info!("EHCI USB allocator virtual addr: 0x{:x}", addr);

            (usb_alloc.into_borrowed_mut::<UsbAlloc>(0).map_err(|(_, msg)| msg)?, four_gig_segment)
        };

        let mut config_space = map_frame_range(base, PAGE_SIZE, MMIO_FLAGS)?;
        let capa_regs = config_space.as_type::<CapabilityRegisters>(0)?;

        let op_offset = capa_regs.cap_length.read() as usize;
        let hcs_params = capa_regs.hcs_params.read();

        let mut op_regs = config_space.into_borrowed_mut::<OperationRegisters>(op_offset).map_err(|(_, msg)| msg)?;

        op_regs.interrupts.update(|i| i.set_usb_int(true));
        op_regs.interrupts.update(|i| i.set_usb_error_int(true));
        op_regs.interrupts.update(|i| i.set_port_change_int(true));
        op_regs.interrupts.update(|i| i.set_frame_list_rollover_int(false));
        op_regs.interrupts.update(|i| i.set_host_system_error_int(true));
        op_regs.interrupts.update(|i| i.set_int_on_async_advance(false));

        op_regs.segment.write(four_gig_segment);
        op_regs.command.update(|cmd| cmd.set_frame_list_size(FrameListSize::Full));
        op_regs.command.update(|cmd| cmd.set_async_schedule(false));
        op_regs.command.update(|cmd| cmd.set_periodic_schedule(false));
        op_regs.command.update(|cmd| cmd.set_run_stop(true));
        op_regs.config_flag.update(|cmd| cmd.set_inner(ConfigureFlag::Use));

        // we're going to prepare some USB structures
        let (dummy_queue_head_addr, periodic_list_addr) = {
            // create a dummy queue head which does nothing
            let first_qtd_none = PointerNoType::from(1);
            let dummy_queue_head = create_queue_head(true, 0, u4::new(0), 0, 0, first_qtd_none);
            let (index, dummy_queue_head_addr) = usb_alloc.queue_heads.allocate(Some(dummy_queue_head))?;

            // link it back to itself, closing the loopy linked list
            let qdh_mut = usb_alloc.queue_heads.get_mut(index)?;
            qdh_mut.next.write(queue_head_pointer(dummy_queue_head_addr));

            // get the address of our periodic list
            let periodic_list_addr = UsbPointer::from_ref(&usb_alloc.periodic_list);
            (dummy_queue_head_addr, periodic_list_addr)
        };

        // this installs a single dummy queue head in the asynchronous schedule,
        // which makes asynchronous queue management easier.
        op_regs.async_list.write(dummy_queue_head_addr.0);

        // set the periodic list pointer (while its schedule is disabled).
        op_regs.periodic_list.write(periodic_list_addr.0);

        sleep_ms(10);

        log::info!("Initialized an EHCI USB controller with {} ports and {} companion controllers",
            hcs_params.port_num(),
            hcs_params.comp_ctrl_num());

        Ok(Self {
            devices: 0,
            hcs_params,
            op_regs,
            usb_alloc,
            pending_int_transfers: Vec::new(),
            interfaces: Vec::new(),
            initialized: false,
        })
    }

    fn init_device(&mut self, addr: DeviceAddress) -> Result<(), &'static str> {
        let mut device = descriptors::Device::default();
        self.request(addr, Request::GetDeviceDescriptor(&mut device), 8)?;

        let max_packet_size = device.max_packet_size as MaxPacketSize;

        let mut config = unsafe { core::mem::MaybeUninit::<descriptors::Configuration>::zeroed().assume_init() };
        self.request(addr, Request::GetConfigDescriptor(0, &mut config), max_packet_size)?;

        let device = (addr, max_packet_size);
        let mut offset = 0;
        for _ in 0..config.inner.num_interfaces {
            let (interface, o): (&descriptors::Interface, _) = config.find_desc(offset, DescriptorType::Interface)?;
            for _ in 0..interface.num_endpoints {
                let interface_id = self.interfaces.len();
                let manager = interfaces::init(self, device, interface, interface_id, &config, o);
                if let Some(interface_mgr) = manager? {
                    self.interfaces.push(Some(interface_mgr));
                }
            }

            offset = o;
        }

        Ok(())
    }

    fn handle_interrupt_transfer(&mut self, dev_addr: DeviceAddress, t: usize) -> Result<bool, &'static str> {
        self.enable_periodic_schedule(false)?;

        let transfer = self.pending_int_transfers[t];
        self.qtd_error_check(transfer.first_qtd_index)?;

        // destroy these transfer descriptors
        self.qh_and_qtd_cleanup(None, transfer.first_qtd_index)?;

        // todo: better ownership management
        let mut interface = self.interfaces[transfer.interface_id].take().unwrap();
        let action = interface.on_interrupt_transfer(self, transfer.endpoint)?;
        assert_eq!(self.interfaces[transfer.interface_id].replace(interface), None);

        match action {
            InterruptTransferAction::Restore => {
                let pid_code = match transfer.endpoint.direction() {
                    Direction::Out => PidCode::Out,
                    Direction::In => PidCode::In,
                };

                let (buffer, data_size) = transfer.buffer;
                let transfer_chain = create_transfer_chain(
                    &mut self.usb_alloc,
                    false /* not sure */,
                    pid_code,
                    buffer,
                    data_size,
                    true,
                )?;

                // cannot fail, would have failed earlier
                let (first_qtd_index, first_qtd_ptr, _) = transfer_chain.unwrap();

                let queue_head = self.usb_alloc.queue_heads.get_mut_by_addr(transfer.queue_head)?;
                *queue_head = create_queue_head(
                    false,
                    dev_addr,
                    transfer.endpoint.ep_number(),
                    0xff,
                    transfer.ep_max_packet_size,
                    first_qtd_ptr.into(),
                );

                self.pending_int_transfers[t].first_qtd_index = first_qtd_index;
                // we didn't actually modify the periodic list; no need to populate it again, but we must restart it.
                self.enable_periodic_schedule(true)?;

                // this transfer still exists
                Ok(true)
            },
            InterruptTransferAction::Destroy => {
                self.usb_alloc.queue_heads.free_by_addr(transfer.queue_head)?;
                self.pending_int_transfers.remove(t);
                self.populate_periodic_schedule()?;

                // this transfer was removed
                Ok(false)
            },
        }
    }

    fn enable_periodic_schedule(&mut self, enable: bool) -> Result<(), &'static str> {
        self.op_regs.command.update(|cmd| cmd.set_periodic_schedule(enable));

        // wait for it to actually stop
        try_wait_until!(2, 100, self.op_regs.status.read().periodic_schedule_running() == enable)?;

        Ok(())
    }

    // Will enable the periodic schedule if needed
    fn populate_periodic_schedule(&mut self) -> Result<(), &'static str> {
        let transfer_count = self.pending_int_transfers.len();
        if transfer_count != 0 {
            let mut transfer_index = 0;
            for entry in &mut self.usb_alloc.periodic_list.entries {
                let transfer = &self.pending_int_transfers[transfer_index];

                transfer_index += 1;
                if transfer_index == transfer_count {
                    transfer_index = 0;
                }

                entry.write(queue_head_pointer(transfer.queue_head));
            }

            self.enable_periodic_schedule(true)?;
        } else {
            // no transfer => don't turn it back on
        }

        Ok(())
    }

    fn turn_off(&mut self) -> Result<(), &'static str> {
        self.op_regs.command.update(|cmd| cmd.set_run_stop(false));
        Ok(())
    }

    fn qtd_error_check(&self, first_qtd_index: AllocSlot) -> Result<(), &'static str> {
        let mut qtd_index = first_qtd_index;
        let no_next = PointerNoType::from(1);

        loop {
            let qtd_ref = self.usb_alloc.transfer_descriptors.get(qtd_index)?;

            let token = qtd_ref.token.read();
            let failure = token.missed_micro_frame()
                       || token.transaction_error()
                       || token.babble_detected()
                       || token.data_buffer_error()
                       || token.halted();

            if failure {
                return Err("Transfer status indicates a failed transfer");
            }

            let next_pointer = qtd_ref.next.read();
            if next_pointer == no_next {
                return Ok(());
            } else {
                let next_addr = next_pointer.address();
                qtd_index = self.usb_alloc.transfer_descriptors.find(next_addr)?;
            }
        }
    }

    fn qh_and_qtd_cleanup(&mut self, qh_addr: Option<UsbPointer>, first_qtd_index: AllocSlot) -> Result<(), &'static str> {
        if let Some(qh_addr) = qh_addr {
            self.usb_alloc.queue_heads.free_by_addr(qh_addr)?;
        }

        let mut qtd_index = first_qtd_index;
        let no_next = PointerNoType::from(1);

        loop {
            let qtd_ref = self.usb_alloc.transfer_descriptors.free(qtd_index)?;

            let next_pointer = qtd_ref.next.read();
            if next_pointer == no_next {
                return Ok(());
            } else {
                let next_addr = next_pointer.address();
                qtd_index = self.usb_alloc.transfer_descriptors.find(next_addr)?;
            }
        }
    }

    fn wait_for_all_td_inactive(&self, first_qtd_index: AllocSlot) -> Result<(), &'static str> {
        let transfer_descriptors = &self.usb_alloc.transfer_descriptors;
        let mut qtd_index = first_qtd_index;
        let no_next = PointerNoType::from(1);

        loop {
            let qtd_ref = transfer_descriptors.get(qtd_index)?;
            try_wait_until!(5, 500, !qtd_ref.token.read().active())?;

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
        self.op_regs.command.update(|cmd| cmd.set_async_schedule(enable));
        try_wait_until!(5, 500, self.op_regs.status.read().async_schedule_running() == enable)?;
        Ok(())
    }

    fn get_async_schedule_prev(&self, queue_head_addr: UsbPointer) -> Result<AllocSlot, &'static str> {
        let queue_heads = &self.usb_alloc.queue_heads;
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

    fn read_async_list_ptr(&self) -> Result<UsbPointer, &'static str> {
        Ok(UsbPointer(self.op_regs.async_list.read()))
    }

    fn push_to_async_schedule(&mut self, to_push_addr: UsbPointer) -> Result<(), &'static str> {
        self.enable_async_schedule(false)?;

        let first_addr = self.read_async_list_ptr()?;
        let last_index = self.get_async_schedule_prev(first_addr)?;

        let queue_heads = &mut self.usb_alloc.queue_heads;

        // set `to_push` as next of `last`
        let last_mut = queue_heads.get_mut(last_index)?;
        last_mut.next.write(queue_head_pointer(to_push_addr));

        // set next of `to_push` to `first`
        let to_push_mut = queue_heads.get_mut_by_addr(to_push_addr)?;
        to_push_mut.next.write(queue_head_pointer(first_addr));

        self.enable_async_schedule(true)
    }

    fn remove_from_async_schedule(&mut self, to_remove_addr: UsbPointer) -> Result<(), &'static str> {
        self.enable_async_schedule(false)?;

        // I'm not sure if the controller can actually advance the pointer in ASYNCLISTADDR.
        // If it can, this will probably fail easily.
        let first_addr = self.read_async_list_ptr()?;
        assert_ne!(first_addr, to_remove_addr, "[USB-EHCI] Tried to remove a queue head while the controller was using it.");

        let prev_index = self.get_async_schedule_prev(to_remove_addr)?;
        let queue_heads = &mut self.usb_alloc.queue_heads;

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
    data_size: usize,
    max_packet_size: MaxPacketSize,
    buffer: UsbPointer,
) -> Result<(UsbPointer, AllocSlot, AllocSlot), &'static str> {
    let (req_index, req_ptr) = shmem.common.requests.allocate(Some(req))?;
    let req_bp = req_ptr.into();

    let zero_bp = BufferPointer::from(0);
    let no_next = PointerNoType::from(1);

    let setup_token = QtdToken::new(
        // initial status flags:
        false, false, false, false, false, false, false,

        true,
        PidCode::Setup,
        u2::new(3),
        u3::new(0),
        false,
        u15::new(size_of::<RawRequest>() as _),
        // setup stage always uses DATA0
        false,
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

    let (setup_qtd_index, setup_qtd_ptr) = shmem.transfer_descriptors.allocate(Some(setup_qtd))?;

    let (data_pid_code, status_pid_code) = match req.direction() {
        Direction::Out => (PidCode::Out, PidCode::In),
        Direction::In => (PidCode::In, PidCode::Out),
    };

    let transfer_chain = create_transfer_chain(
        shmem,
        true, // data stage builds upon setup stage
        data_pid_code,
        buffer,
        data_size,
        false,
    )?;

    let last_qtd_index = match transfer_chain {
        Some(tuple) => {
            let (_, first_data_qtd_ptr, last_qtd_index) = tuple;

            // link first data qtd
            let setup_qtd = shmem.transfer_descriptors.get_mut(setup_qtd_index)?;
            setup_qtd.next.write(first_data_qtd_ptr.into());

            last_qtd_index
        },
        None => setup_qtd_index,
    };

    let status_token = QtdToken::new(
        // initial status flags:
        false, false, false, false, false, false, false,
        true,
        status_pid_code,
        u2::new(3),
        u3::new(0),
        false,
        u15::new(0),
        // status stage always uses DATA1
        true,
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

    // link setup qtd
    let prev_qtd = shmem.transfer_descriptors.get_mut(last_qtd_index)?;
    prev_qtd.next.write(status_qtd_ptr.into());

    let qh = create_queue_head(false, dev_addr, u4::new(0), 0, max_packet_size, setup_qtd_ptr.into());
    let (_qh_index, queue_head_addr) = shmem.queue_heads.allocate(Some(qh))?;

    Ok((queue_head_addr, setup_qtd_index, req_index))
}

fn create_int_transfer_qh(
    shmem: &mut UsbAlloc,
    dev_addr: DeviceAddress,
    endpoint: EndpointAddress,
    max_packet_size: MaxPacketSize,
    buffer: UsbPointer,
    data_size: usize,
) -> Result<(UsbPointer, AllocSlot), &'static str> {
    let pid_code = match endpoint.direction() {
        Direction::Out => PidCode::Out,
        Direction::In => PidCode::In,
    };

    let transfer_chain = create_transfer_chain(
        shmem,
        false /* not sure */,
        pid_code,
        buffer,
        data_size,
        true,
    )?;

    let (first_qtd_index, first_qtd_ptr) = match transfer_chain {
        Some((first_qtd_index, first_qtd_ptr, _)) => (first_qtd_index, first_qtd_ptr),
        None => return Err("Cannot create an interrupt transfer with zero data size"),
    };

    let first_qtd_ptr = PointerNoType::from(first_qtd_ptr);
    let qh = create_queue_head(false, dev_addr, endpoint.ep_number(), 0xff, max_packet_size, first_qtd_ptr);
    let (_qh_index, queue_head_addr) = shmem.queue_heads.allocate(Some(qh))?;

    Ok((queue_head_addr, first_qtd_index))
}

fn create_transfer_chain(
    shmem: &mut UsbAlloc,
    initial_data_toggle: bool,
    pid_code: PidCode,
    buffer: UsbPointer,
    mut data_size: usize,
    int_on_complete: bool,
) -> Result<Option<(AllocSlot, UsbPointer, AllocSlot)>, &'static str> {
    let no_next = PointerNoType::from(1);
    let mut data_toggle = initial_data_toggle;
    let mut buffer = buffer.0;

    // these values will be overwritten
    let (mut first_qtd_index, mut first_qtd_ptr) = invalid_ptr_slot();
    let mut prev_qtd_index = None;

    while data_size > 0 {
        // Refer to section 4.10.6 for these computations
        let over_align = buffer % 0x1000;
        let first_buffer_len = match over_align {
            0 => 0x1000, // buffer is aligned, no need to align the next one
            over_align => 0x1000 - over_align, // bp1, bp2 etc will be aligned to 4KiB
        };

        let max_transfer_progress = 0x4000 + (first_buffer_len as usize);
        let last_transfer = data_size <= max_transfer_progress;
        let progress = match last_transfer {
            true => data_size,
            false => max_transfer_progress,
        };

        let in_token = QtdToken::new(
            false, false, false, false, false, false, false, // initial status/error flags
            true, // active
            pid_code,
            u2::new(3),
            u3::new(0),
            int_on_complete && last_transfer,
            u15::new(progress as _),
            // data stage chains DATA0 with DATA1 and vice versa
            data_toggle,
        );

        // these might not all be used by the controller
        // if `progress` is less than 5 x 4KiB
        let bp0 = UsbPointer(buffer).into();
        let bp1 = UsbPointer(buffer + first_buffer_len).into();
        let bp2 = UsbPointer(buffer + first_buffer_len + 0x1000).into();
        let bp3 = UsbPointer(buffer + first_buffer_len + 0x2000).into();
        let bp4 = UsbPointer(buffer + first_buffer_len + 0x3000).into();

        let in_qtd = TransferDescriptor {
            next: Volatile::new(no_next),
            alt_next: Volatile::new(no_next),
            token: Volatile::new(in_token),
            bp0: Volatile::new(bp0),
            bp1: Volatile::new(bp1),
            bp2: Volatile::new(bp2),
            bp3: Volatile::new(bp3),
            bp4: Volatile::new(bp4),
        };

        let (part_qtd_index, part_qtd_ptr) = shmem.transfer_descriptors.allocate(Some(in_qtd))?;

        if let Some(prev_qtd_index) = prev_qtd_index {
            let prev_qtd = shmem.transfer_descriptors.get_mut(prev_qtd_index)?;
            prev_qtd.next.write(PointerNoType::from(part_qtd_ptr));
        } else {
            first_qtd_index = part_qtd_index;
            first_qtd_ptr = part_qtd_ptr;
        }

        prev_qtd_index = Some(part_qtd_index);

        data_toggle = !data_toggle;
        buffer += progress as u32;
        data_size -= progress;
    }

    Ok(prev_qtd_index.map(|last_qtd_index| (first_qtd_index, first_qtd_ptr, last_qtd_index)))
}

fn create_queue_head(
    is_first_qh: bool,
    dev_addr: DeviceAddress,
    endpoint: u4,
    int_schedule_mask: u8,
    max_packet_size: MaxPacketSize,
    first_qtd_bp: PointerNoType,
) -> QueueHead {
    let qh_endpoint = QhEndpoint::new(
        u7::new(dev_addr),
        false,
        endpoint,
        EndpointSpeed::HighSpeed,
        true,
        is_first_qh,
        u11::new(max_packet_size as _),
        // must only be set for LS/FS control endpoints
        false,
        u4::new(0),
    );

    let qh_uframe = QhMicroFrame::new(
        int_schedule_mask, 0, u7::new(0), u7::new(0),
        HighBandwidthPipeMultiplier::One,
    );

    // note: for interrupt transfers, this may remain an invalid pointer
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

fn queue_head_pointer(queue_head_addr: UsbPointer) -> Pointer {
    Pointer::new(false, PointerType::QueueHead, u27::new(queue_head_addr.0 >> 5))
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

#[derive(Debug, FromBytes)]
#[repr(C)]
#[repr(align(4096))]
struct PeriodicList {
    entries: [Volatile<Pointer>; PERIODIC_LIST_LEN],
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

impl From<UsbPointer> for PointerNoTypeNoTerm {
    fn from(ptr: UsbPointer) -> Self {
        assert_eq!(ptr.0 & 0b11111, 0, "UsbPointer alignment incompatible with PointerNoTypeNoTerm");
        Self::from(ptr.0)
    }
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes, PartialEq)]
struct PointerNoType {
    invalid: bool,
    reserved: u4,
    addr_msbs: u27,
}

impl From<UsbPointer> for PointerNoType {
    fn from(ptr: UsbPointer) -> Self {
        assert_eq!(ptr.0 & 0b11111, 0, "UsbPointer alignment incompatible with PointerNoType");
        Self::from(ptr.0)
    }
}

impl PointerNoType {
    fn address(&self) -> UsbPointer {
        UsbPointer(self.addr_msbs().value() << 5)
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

impl From<UsbPointer> for Pointer {
    fn from(ptr: UsbPointer) -> Self {
        assert_eq!(ptr.0 & 0b11111, 0, "UsbPointer alignment incompatible with ehci::Pointer");
        Self::from(ptr.0)
    }
}

impl Pointer {
    fn address(&self) -> UsbPointer {
        UsbPointer(self.addr_msbs().value() << 5)
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

impl From<UsbPointer> for BufferPointerWithOffset {
    fn from(ptr: UsbPointer) -> Self {
        Self::from(ptr.0)
    }
}

#[bitsize(32)]
#[derive(DebugBits, Copy, Clone, FromBits, FromBytes)]
struct BufferPointer {
    reserved: u12,
    _ptr: u20,
}

impl From<UsbPointer> for BufferPointer {
    fn from(ptr: UsbPointer) -> Self {
        assert_eq!(ptr.0 & 0xfff, 0, "UsbPointer alignment incompatible with BufferPointer");
        Self::from(ptr.0)
    }
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
    // must only be set for LS/FS control endpoints
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
