
struct InitializationSegment {
    fw_rev_major: u16,
    fw_rev_minor: u16,
    fw_rev_subminor: u16,
    cmd_interface_rev: u16,
    cmdq_phy_addr: u32,
    log_cmdq_stride_size-nic_interface-cmdq_phy_addr: u32,
    command_doorbell_vector: u32,
    _padding: [u8;x]
}