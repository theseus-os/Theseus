# irq_safety: Interrupt-safe spinlock Mutex/RwLock

Irq-safe locking via Mutex and RwLock. 

Offers identical behavior to the regular `spin` crate's Mutex and RwLock,
with the added behavior of holding interrupts for the duration of the Mutex guard. 

When the lock guard is dropped (falls out of scope), interrupts are re-enabled 
if and only if they were enabled when the lock was obtained. 

Also provides a interrupt holding feature without locking, if desired. Interrupt safety is achieved simply by disabling interrupts, which currently works for x86 architectures only. 