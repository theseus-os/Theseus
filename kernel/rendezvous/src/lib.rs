//! A rendezvous-based channel for synchronous Inter-Task Communication (ITC).
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
extern crate mutex_sleep;

use core::fmt;
use alloc::sync::Arc;
use irq_safety::MutexIrqSafe;
use spin::Mutex;
use wait_queue::WaitGuard;

// struct InnerSlot<T>(MutexIrqSafe<ExchangeState<T>>);

/// 
struct ExchangeSlot<T> {
    sender:   Option<Arc<MutexIrqSafe<ExchangeState<T>>>>,
    receiver: Option<Arc<MutexIrqSafe<ExchangeState<T>>>>,
}
impl<T> ExchangeSlot<T> {
    fn new() -> ExchangeSlot<T> {
        let inner = Arc::new(MutexIrqSafe::new(ExchangeState::Init));
        ExchangeSlot {
            sender:   Some(inner.clone()),
            receiver: Some(inner),
        }
    }

    // fn take_sender_slot(&mut self) -> ExchangeSlotGuard<T> {
    //     unimplemented!()
    // }

    // fn take_receiver_slot(&mut self) -> ExchangeSlotGuard<T> {
    //     unimplemented!()
    // }
}


enum ExchangeState<T> {
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
impl<T> fmt::Debug for ExchangeState<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ExchangeState::{}", match self {
            ExchangeState::Init => "Init",
            ExchangeState::WaitingForReceiver(..) => "WaitingForReceiver",
            ExchangeState::WaitingForSender(..) => "WaitingForSender",
            ExchangeState::ReceiverFinishedFirst => "ReceiverFinishedFirst",
            ExchangeState::SenderFinishedFirst(..) => "SenderFinishedFirst",
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
        slot: Mutex::new(ExchangeSlot::new()),
    });
    (Sender::new(channel.clone()), Receiver::new(channel))
}



// struct ExchangeSlotGuard<'m, T> {
//     is_sender: bool,
//     slot: Arc<MutexIrqSafe<ExchangeState<T>>>,
//     inner_slot_ref: &'m Mutex<ExchangeSlot<T>>,
// }
// impl<'s, T> Drop for ExchangeSlotGuard<'s, T> {
//     fn drop(&mut self) {
//         let newly_available_slot = core::mem::replace(&mut self.slot, Arc::default());
//         if is_sender {
//             *self.inner_slot_ref.lock().sender = Some(newly_available_slot);
//         } else {
//             *self.inner_slot_ref.lock().receiver = Some(newly_available_slot);
//         }
//     }
// }



/// The inner channel for synchronous, rendezvous-based communication
/// between `Sender`s and `Receiver`s.
struct Channel<T: Send> {
    /// In a zero-capacity synchronous channel, there is only a single slot,
    /// but senders and receivers perform a blocking wait on it until the slot becomes available.
    /// In contrast, a synchronous channel with a capacity of 1 would return a "channel full" error
    /// if the slot was taken, instead of blocking. 
    slot: Mutex<ExchangeSlot<T>>,
}
// impl<T: Send> Channel<T> {
//     fn take_sender_slot(&mut self) -> ExchangeSlotGuard<T> {
//         loop {
//             if let Some(s) = self.channel.slot.lock().sender.take() {
//                 return ExchangeSlotGuard {

//                 };
//             }
//             scheduler::schedule();
//         }
//     }

//     fn take_receiver_slot(&mut self) -> ExchangeSlotGuard<T> {
//         unimplemented!()
//     }
// }


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

        // obtain a sender-side exchange slot, blocking if necessary
        let slot_ref = loop {
            trace!("Sender is waiting for slot to become available...");
            if let Some(s) = self.channel.slot.lock().sender.take() {
                break s;
            }
            trace!("Sender slot was taken...");
            scheduler::schedule();
        };

        // Here, either the sender (this task) arrived first and needs to wait for a receiver,
        // or a receiver has already arrived and is waiting for a sender. 
        {
            let mut slot = slot_ref.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *slot, ExchangeState::Init);
            match current_state {
                ExchangeState::Init => {
                    // Hold interrupts to avoid blocking & descheduling this task until we release the slot lock,
                    // which is currently done automatically because the slot uses a MutexIrqSafe.
                    *slot = ExchangeState::WaitingForReceiver(WaitGuard::new(curr_task.clone()), msg);
                }
                ExchangeState::WaitingForSender(receiver_to_nofity) => {
                    *slot = ExchangeState::SenderFinishedFirst(msg);
                    // The message has been sent successfully! Notify the receiver task.
                    drop(receiver_to_nofity);
                    // Note that here we do NOT return/restore the sender slot to the channel yet; 
                    // that will be done once the receiver is also finished with the slot (in SenderFinishedFirst).
                    return Ok(());
                }
                other => {
                    error!("BUG: Sender (at beginning) in invalid state {:?}", other);
                    *slot = other;
                    // restore the sender slot
                    self.channel.slot.lock().sender = Some(slot_ref.clone());
                    return Err("BUG: Sender (at beginning) in invalid state");
                }
            }
            // here, the waiter lock is dropped
        }
        scheduler::schedule();

        // Here, the sender (this task) is waiting for a receiver
        loop {
            {
                let slot = slot_ref.lock();
                match &*slot {
                    ExchangeState::WaitingForReceiver(blocked_sender, ..) => {
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
        let retval = {
            let mut slot = slot_ref.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *slot, ExchangeState::Init);
            match current_state {
                ExchangeState::ReceiverFinishedFirst => {
                    *slot = ExchangeState::Init; // ready to transfer another message.
                    // restore the receiver slot now that the receiver is finished.
                    self.channel.slot.lock().receiver = Some(slot_ref.clone());
                    Ok(())
                }
                other => {
                    error!("BUG: Sender (while waiting) in invalid state {:?}", other);
                    *slot = other;
                    Err("BUG: Sender (while waiting) in invalid state")
                }
            }
        };
        
        // restore the sender slot now that we're completely done with it
        trace!("sender done, restoring slot");
        self.channel.slot.lock().sender = Some(slot_ref.clone());
        retval
        
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
        
        // obtain a receiver-side exchange slot, blocking if necessary
        let slot_ref = loop {
            trace!("Receiver is waiting for slot to become available...");
            if let Some(r) = self.channel.slot.lock().receiver.take() {
                break r;
            }
            trace!("Receiver slot was taken...");
            scheduler::schedule();
        };

        // Here, either the receiver (this task) arrived first and needs to wait for a sender,
        // or a sender has already arrived and is waiting for a receiver. 
        {
            let mut slot = slot_ref.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *slot, ExchangeState::Init);
            match current_state {
                ExchangeState::Init => {
                    // Hold interrupts to avoid blocking & descheduling this task until we release the slot lock,
                    // which is currently done automatically because the slot uses a MutexIrqSafe.
                    *slot = ExchangeState::WaitingForSender(WaitGuard::new(curr_task.clone()));
                }
                ExchangeState::WaitingForReceiver(sender_to_notify, msg) => {
                    *slot = ExchangeState::ReceiverFinishedFirst;
                    // The message has been received successfully! Notify the sender task.
                    drop(sender_to_notify);
                    // Note that here we do NOT return/restore the sender slot to the channel yet; 
                    // that will be done once the sender is also finished with the slot (in ReceiverFinishedFirst).
                    return Ok(msg);
                }
                other => {
                    error!("BUG: Receiver (at beginning) in invalid state {:?}", other);
                    *slot = other;
                    // restore the receiver slot
                    self.channel.slot.lock().receiver = Some(slot_ref.clone());
                    return Err("BUG: Receiver (at beginning) in invalid state");
                }
            }
            // here, the waiter lock is dropped
        }
        scheduler::schedule();

        // Here, the receiver (this task) is waiting for a sender
        loop {
            {
                let slot = slot_ref.lock();
                match &*slot {
                    ExchangeState::WaitingForSender(blocked_receiver) => {
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
        let retval = {
            let mut slot = slot_ref.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *slot, ExchangeState::Init);
            match current_state {
                ExchangeState::SenderFinishedFirst(msg) => {
                    *slot = ExchangeState::Init; // ready to transfer another message.
                    // restore the sender slot now that the sender is finished.
                    self.channel.slot.lock().sender = Some(slot_ref.clone());
                    Ok(msg)
                }
                other => {
                    error!("BUG: Receiver (at end) in invalid state {:?}", other);
                    *slot = other;
                    Err("BUG: Receiver (at end) in invalid state")
                }
            }
        };

        // restore the receiver slot now that we're completely done with it
        trace!("receiver done, restoring slot");
        self.channel.slot.lock().receiver = Some(slot_ref.clone());
        retval
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