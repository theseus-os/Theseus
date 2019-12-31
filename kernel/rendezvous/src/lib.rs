//! Synchronous Inter-Task Communication (ITC) using a rendezvous approach.
//! 
//! This crate offers a rendezvous channel, in which two tasks can exchange messages
//! without an intermediary buffer. 
//! The sender and receiver tasks must rendezvous together to exchange data,
//! so at least one of them must block. 
//! 
//! Only `Send` types can be sent or received through the channel.
//! 
//! This is not a zero-copy channel; 
//! To avoid copying large messages, use a reference (layer of indirection) like `Box`.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate wait_queue;
extern crate task;
extern crate scheduler;
// extern crate mutex_sleep;

use core::fmt;
use alloc::sync::Arc;
use irq_safety::MutexIrqSafe;
// use mutex_sleep::MutexSleep;
use wait_queue::WaitGuard;


enum WaitEntry<T> {
    /// Initial state: we're waiting for either a sender or a receiver.
    Init,
    /// A sender has arrived. 
    /// The `WaitGuard` contains the blocked sender task,
    /// and the `T` is the message that will be exchanged.
    WaitingForReceiver(WaitGuard, T),
    /// A receiver has arrived.
    /// The `WaitGuard` contains the blocked receiver task.
    WaitingForSender(WaitGuard),
    /// Sender and Receiver have rendezvoused, but the receiver finished first.
    /// Thus, it is the sender's responsibility to reset to the initial state.
    ReceiverFinishedFirst,
    /// Sender and Receiver have rendezvoused, but the sender finished first.
    /// Thus, the message `T` is enclosed here for the receiver to take, 
    /// and it is the receivers's responsibility to reset to the initial state.
    SenderFinishedFirst(T),
}
impl<T> fmt::Debug for WaitEntry<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WaitEntry::{}", match self {
            WaitEntry::Init => "Init",
            WaitEntry::WaitingForReceiver(..) => "WaitingForReceiver",
            WaitEntry::WaitingForSender(..) => "WaitingForSender",
            WaitEntry::ReceiverFinishedFirst => "ReceiverFinishedFirst",
            WaitEntry::SenderFinishedFirst(..) => "SenderFinishedFirst",
        })
    }
}

// enum RendezvousState<T> {
//     /// Initial state: we're waiting for either a sender or a receiver.
//     Init,
//     /// A task is blocked and waiting to rendezvous; the blocked task is held in the `WaitGuard`.
//     /// The `Option<T>` is for exchanging the message, and indicates whether the blocked task is a sender or receiver.
//     /// * If `None`, then the receiver is blocked, waiting on a sender to put its message into `Some(T)`.
//     /// * If `Some`, then the sender is blocked, waiting on a receiver to take the message out of `Option<T>`.
//     Waiting(WaitGuard, Option<T>),
// }


pub fn new_channel<T: Send>() -> (Sender<T>, Receiver<T>) {
    let channel = Arc::new(Channel::<T> {
        waiter: MutexIrqSafe::new(WaitEntry::Init),
    });
    (Sender::new(channel.clone()), Receiver::new(channel))
}


/// The inner channel for synchronous, rendezvous-based communication
/// between `Sender`s and `Receiver`s.
struct Channel<T: Send> {
    waiter: MutexIrqSafe<WaitEntry<T>>,
}


pub struct Sender<T: Send> {
    channel: Arc<Channel<T>>,
}
impl <T: Send> Sender<T> {
    fn new(channel: Arc<Channel<T>>) -> Sender<T> {
        Sender {
            channel,
        }
    }

    /// Send a message, blocking until a receiver is ready.
    /// 
    /// Returns `Ok(())` if the message was sent and received successfully,
    /// otherwise returns an error. 
    pub fn send(&self, msg: T) -> Result<(), &'static str> {
        let curr_task = task::get_my_current_task().ok_or("couldn't get current task")?;

        // TODO: block on obtaining a wait slot with a valid state: either Init or WaitingForSender

        // Here, either the sender (us) arrived first and needs to wait for a receiver,
        // or a receiver has already arrived and is waiting for a sender. 
        {
            let _held_interrupts = {
                let mut wait_entry = self.channel.waiter.lock();
                // Temporarily take ownership of the channel's waiting state so we can modify it;
                // the match statement below will advance the waiting state to the proper next state.
                let current_state = core::mem::replace(&mut *wait_entry, WaitEntry::Init);
                match current_state {
                    WaitEntry::Init => {
                        // hold interrupts to avoid blocking & descheduling this task until we release the waiter lock.
                        let held_interrupts = irq_safety::hold_interrupts();
                        *wait_entry = WaitEntry::WaitingForReceiver(WaitGuard::new(curr_task.clone()), msg);
                        held_interrupts
                    }
                    WaitEntry::WaitingForSender(receiver_to_nofity) => {
                        *wait_entry = WaitEntry::SenderFinishedFirst(msg);
                        drop(receiver_to_nofity); // notifies the receiver
                        // the message has been sent successfully!
                        return Ok(());
                    }
                    other => {
                        error!("BUG: Sender (at beginning) in invalid state {:?}", other);
                        *wait_entry = other;
                        return Err("BUG: Sender (at beginning) in invalid state");
                    }
                }
                // here, the waiter lock is dropped
            };
            // here, interrupts are re-enabled and this task (the sender) will be descheduled
        }

        // Here, the sender (us) is waiting for a receiver
        loop {
            {
                let wait_entry = self.channel.waiter.lock();
                match &*wait_entry {
                    WaitEntry::WaitingForReceiver(blocked_sender, ..) => {
                        warn!("spurious wakeup while sender is waiting for receiver... re-blocking task.");
                        if blocked_sender.task() != curr_task {
                            return Err("BUG: CURR TASK WAS DIFFERENT THAN BLOCKED SENDER");
                        }
                        blocked_sender.block_again();
                    }
                    _ => break,
                }
            }
            scheduler::schedule();
        }

        // Here, we are at the rendezvous point
        let mut wait_entry = self.channel.waiter.lock();
        // Temporarily take ownership of the channel's waiting state so we can modify it;
        // the match statement below will advance the waiting state to the proper next state.
        let current_state = core::mem::replace(&mut *wait_entry, WaitEntry::Init);
        match current_state {
            WaitEntry::ReceiverFinishedFirst => {
                *wait_entry = WaitEntry::Init; // start over: ready to transfer another message.
                Ok(())
            }
            other => {
                error!("BUG: Sender (while waiting) in invalid state {:?}", other);
                *wait_entry = other;
                Err("BUG: Sender (while waiting) in invalid state")
            }
        }


        
        /*
        loop {
            let mut wait_entry = self.channel.waiter.lock();
            // temporarily take ownership of the channel's waiting state so we can modify it.
            let current_state = core::mem::replace(&mut *wait_entry, RendezvousState::Init);
            match current_state {
                RendezvousState::Init => {
                    let _held_interrupts = irq_safety::hold_interrupts();
                    *wait_entry = RendezvousState::Waiting(WaitGuard::new(curr_task.clone()), Some(msg));
                    // interrupts are re-enabled here
                }
                RendezvousState::Waiting(task_to_nofity, dest) => {
                    *dest = Some(msg);
                    let _held_interrupts = irq_safety::hold_interrupts();
                    *task_to_nofity = WaitGuard::new(curr_task.clone());
                    drop(task_to_nofity); // notifies the receiver
                }
            };
            let old_state = core::mem::replace(&mut wait_entry, new_state);
        }
        */
    }

    /// Tries to send the message, only succeeding if a receiver is ready and waiting. 
    /// 
    /// If a receiver was not ready, it returns the `msg` back to the caller without blocking. 
    /// 
    /// Note that if the non-blocking `try_send` and `try_receive` functions are only ever used,
    /// then the message will never be delivered because the sender and receiver cannot possibly rendezvous. 
    pub fn try_send(&self, _msg: T) -> Result<(), T> {
        unimplemented!()
    }
}

pub struct Receiver<T: Send> {
    channel: Arc<Channel<T>>,
}
impl <T: Send> Receiver<T> {
    fn new(channel: Arc<Channel<T>>) -> Receiver<T> {
        Receiver {
            channel,
        }
    }

    /// Receive a message, blocking until a sender is ready. 
    /// 
    /// Returns the message if it was received properly,
    /// otherwise returns an error.
    pub fn receive(&self) -> Result<T, &'static str> {
        let curr_task = task::get_my_current_task().ok_or("couldn't get current task")?;
        
        // TODO: block on obtaining a wait slot with a valid state: either Init or WaitingForSender

        // Here, either the receiver (us) arrived first and needs to wait for a sender,
        // or a sender has already arrived and is waiting for a receiver. 
        {
            let _held_interrupts = {
                let mut wait_entry = self.channel.waiter.lock();
                // Temporarily take ownership of the channel's waiting state so we can modify it;
                // the match statement below will advance the waiting state to the proper next state.
                let current_state = core::mem::replace(&mut *wait_entry, WaitEntry::Init);
                match current_state {
                    WaitEntry::Init => {
                        // hold interrupts to avoid blocking & descheduling this task until we release the waiter lock.
                        let held_interrupts = irq_safety::hold_interrupts();
                        *wait_entry = WaitEntry::WaitingForSender(WaitGuard::new(curr_task.clone()));
                        held_interrupts
                    }
                    WaitEntry::WaitingForReceiver(sender_to_notify, msg) => {
                        *wait_entry = WaitEntry::ReceiverFinishedFirst;
                        drop(sender_to_notify); // notifies the sender
                        return Ok(msg);
                    }
                    _x => {
                        error!("BUG: Receiver (at beginning) in invalid state {:?}", _x);
                        return Err("BUG: Receiver (at beginning) in invalid state");
                    }
                }
                // here, the waiter lock is dropped
            };
            // here, interrupts are re-enabled and this task (the sender) will be descheduled
        }

        // Here, the receiver (us) is waiting for a sender
        loop {
            {
                let wait_entry = self.channel.waiter.lock();
                match &*wait_entry {
                    WaitEntry::WaitingForSender(blocked_receiver) => {
                        warn!("spurious wakeup while receiver is WaitingForSender... re-blocking task.");
                        if blocked_receiver.task() != curr_task {
                            return Err("BUG: CURR TASK WAS DIFFERENT THAN BLOCKED RECEIVER");
                        }
                        blocked_receiver.block_again();
                    }
                    _ => break,
                }
            }
            scheduler::schedule();
        }


        // Here, we are at the rendezvous point
        let mut wait_entry = self.channel.waiter.lock();
        // Temporarily take ownership of the channel's waiting state so we can modify it;
        // the match statement below will advance the waiting state to the proper next state.
        let current_state = core::mem::replace(&mut *wait_entry, WaitEntry::Init);
        match current_state {
            WaitEntry::SenderFinishedFirst(msg) => {
                *wait_entry = WaitEntry::Init;
                Ok(msg)
            }
            other => {
                error!("BUG: Receiver (at end) in invalid state {:?}", other);
                *wait_entry = other;
                Err("BUG: Receiver (at end) in invalid state")
            }
        }
    }

    /// Tries to receive a message, only succeeding if a sender is ready and waiting. 
    /// 
    /// If the sender was not ready, it returns an error without blocking. 
    /// 
    /// Note that if the non-blocking `try_send` and `try_receive` functions are only ever used,
    /// then the message will never be delivered because the sender and receiver cannot possibly rendezvous. 
    pub fn try_receive(&self) -> Result<T, &'static str> {
        unimplemented!()
    }
}



// TODO: implement drop for sender and receiver in order to notify the other side of a disconnect