use crate::prevention::{private::Sealed, DeadlockPrevention };

pub struct Spin {}

impl Sealed for Spin {}

impl DeadlockPrevention for Spin {
    type Guard = ();

    fn enter() -> Self::Guard {
        ()
    }
}
