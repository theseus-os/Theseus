window.SIDEBAR_ITEMS = {"fn":[["disable_interrupts",""],["enable_interrupts",""],["hold_interrupts","Prevent interrupts from firing until the return value is dropped (goes out of scope). After it is dropped, the interrupts are returned to their prior state, not blindly re-enabled."],["interrupts_enabled",""]],"struct":[["HeldInterrupts","A handle for frozen interrupts"],["MutexIrqSafe","This type provides interrupt-safe MUTual EXclusion based on [spin::Mutex]."],["MutexIrqSafeGuard","A guard to which the protected data can be accessed"],["RwLockIrqSafe","A simple wrapper around a `RwLock` whose guards disable interrupts properly "],["RwLockIrqSafeReadGuard","A guard to which the protected data can be read"],["RwLockIrqSafeWriteGuard","A guard to which the protected data can be written"]],"type":[["MutexIrqSafeGuardRef","Typedef of a owning reference that uses a `MutexIrqSafeGuard` as the owner."],["MutexIrqSafeGuardRefMut","Typedef of a mutable owning reference that uses a `MutexIrqSafeGuard` as the owner."],["RwLockIrqSafeReadGuardRef","Typedef of a owning reference that uses a `RwLockIrqSafeReadGuard` as the owner."],["RwLockIrqSafeWriteGuardRefMut","Typedef of a mutable owning reference that uses a `RwLockIrqSafeWriteGuard` as the owner."]]};