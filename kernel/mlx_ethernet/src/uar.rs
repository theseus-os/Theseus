use zerocopy::*;
use volatile::Volatile;
use byteorder::BigEndian;

#[derive(FromBytes)]
#[repr(C)]
pub(crate) struct UserAccessRegion {
    _padding0:  [u8; 32],
    cq_ci: Volatile<U32<BigEndian>>,
    cq_n: Volatile<U32<BigEndian>>,
    _padding1: [u8; 24],
    eqn1: Volatile<U32<BigEndian>>,
    _padding2: [u8; 4],
    eqn2: Volatile<U32<BigEndian>>,
    _padding3: [u8; 1972],
    pub(crate) db_blueflame_buffer0_even: Volatile<[U32<BigEndian>; 64]>,
    pub(crate) db_blueflame_buffer0_odd: Volatile<[U32<BigEndian>; 64]>,
    db_blueflame_buffer1_even: Volatile<[u8; 256]>,
    db_blueflame_buffer1_odd: Volatile<[u8; 256]>,
}

const_assert_eq!(core::mem::size_of::<UserAccessRegion>(), 3072);
