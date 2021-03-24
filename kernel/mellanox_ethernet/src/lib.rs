 #![no_std]

// #[macro_use]extern crate log;
extern crate memory;
extern crate volatile;
extern crate bit_field;
extern crate zerocopy;

const MAX_CMND_QUEUE_ENTRIES: usize = 64;

struct CommandQueue {
    entries: [CommandQueueEntry, MAX_CMND_QUEUE_ENTRIES]
}

struct CommandQueueEntry {
    type: u32,
    input_length: u32,
    input_mailbox_pointer: u64,
    command_input_inline_data: u32,
    command_output_inline_data: u32,
    output_mailbox_pointer: u64,
    output_length: u32,
    status: u8,
    _padding: u8,
    signature: u8,
    token: u8
}

struct UARPageFormat {
    _padding1: u32, //0x00 - 0x1C
    cmds_cq_ci: u32,
    cqn: u32,
    _padding2: 

}