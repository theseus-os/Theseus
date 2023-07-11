//! User Access Regions (UAR) are used to provide isolated and direct access to the HCA HW to multiple processes.
//! Each UAR is a page within the PCI address space and can be used by a process to post execution and control requests to the HCA.
//! When creating a control object, a UAR page is associated with it. When executing a control operation, the HCA checks that the
//! UAR page used to post the command matches the one specified in the object's context.
//! 
//! (PRM Section 8.2: User Access Region)
use zerocopy::{U32, FromBytes};
use volatile::Volatile;
use byteorder::BigEndian;
use crate::send_queue::CurrentUARDoorbell;

/// The layout of registers within one UAR page.
/// Send DoorBells are rung by writing the first 8 bytes of the WQE to blueflame register 0
/// 
/// (PRM Section 8.2.2: UAR Page Format)
#[derive(FromBytes)]
#[repr(C)]
pub(crate) struct UserAccessRegion {
    _padding0:  [u8; 32],
    /// consumer index of the CQ
    cq_ci: Volatile<U32<BigEndian>>,
    /// CQ number
    cqn: Volatile<U32<BigEndian>>,
    _padding1: [u8; 24],
    /// EQ number to update its ci and arm 
    eqn_with_arm: Volatile<U32<BigEndian>>,
    _padding2: [u8; 4],
    /// EQ number to update its ci
    eqn: Volatile<U32<BigEndian>>,
    _padding3: [u8; 1972],
    /// Doorbell blueflame register of buffer 0 even
    db_blueflame_buffer0_even: Volatile<[U32<BigEndian>; 64]>,
    /// Doorbell blueflame register of buffer 0 odd
    db_blueflame_buffer0_odd: Volatile<[U32<BigEndian>; 64]>,
    /// Doorbell blueflame register of buffer 1 even
    db_blueflame_buffer1_even: Volatile<[U32<BigEndian>; 64]>,
    /// Doorbell blueflame register of buffer 1 odd
    db_blueflame_buffer1_odd: Volatile<[U32<BigEndian>; 64]>,
    /// Doorbell blueflame register of buffer 2 even
    db_blueflame_buffer2_even_fast_path: Volatile<[U32<BigEndian>; 64]>,
    /// Doorbell blueflame register of buffer 2 odd
    db_blueflame_buffer2_odd_fast_path: Volatile<[U32<BigEndian>; 64]>,
    /// Doorbell blueflame register of buffer 3 even
    db_blueflame_buffer3_even_fast_path: Volatile<[U32<BigEndian>; 64]>,
    /// Doorbell blueflame register of buffer 3 odd
    db_blueflame_buffer3_odd_fast_path: Volatile<[U32<BigEndian>; 64]>,
}

const _: () = assert!(core::mem::size_of::<UserAccessRegion>() == 4096);

impl UserAccessRegion {
    pub(crate) fn write_wqe_to_doorbell(&mut self, current_doorbell: &CurrentUARDoorbell, wqe_value: [U32<BigEndian>; 64]) {
        match current_doorbell {
            CurrentUARDoorbell::Even => self.db_blueflame_buffer0_even.write(wqe_value),
            CurrentUARDoorbell::Odd => self.db_blueflame_buffer0_odd.write(wqe_value),
        }
    }
}

impl Default for UserAccessRegion {
    /// We have to define our own default function since only array sizes up to 32 are supported
    fn default() -> UserAccessRegion {
        UserAccessRegion {
            _padding0:  [0; 32],
            cq_ci: Volatile::new(U32::new(0)),
            cqn: Volatile::new(U32::new(0)),
            _padding1: [0; 24],
            eqn_with_arm: Volatile::new(U32::new(0)),
            _padding2: [0; 4],
            eqn: Volatile::new(U32::new(0)),
            _padding3: [0; 1972],
            db_blueflame_buffer0_even: Volatile::new([U32::new(0); 64]),
            db_blueflame_buffer0_odd: Volatile::new([U32::new(0); 64]),
            db_blueflame_buffer1_even: Volatile::new([U32::new(0); 64]),
            db_blueflame_buffer1_odd: Volatile::new([U32::new(0); 64]),
            db_blueflame_buffer2_even_fast_path: Volatile::new([U32::new(0); 64]),
            db_blueflame_buffer2_odd_fast_path: Volatile::new([U32::new(0); 64]),
            db_blueflame_buffer3_even_fast_path: Volatile::new([U32::new(0); 64]),
            db_blueflame_buffer3_odd_fast_path: Volatile::new([U32::new(0); 64]),
        }
    }
}
