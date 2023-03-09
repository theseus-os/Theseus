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
//! 
//! TODO: add support for a queue of pending senders and receivers 
//!       so that we can enable MPMC (multi-producer multi-consumer) behavior
//!       that allows senders and receivers to be cloned. 
//!       Note that currently only a single receiver and single sender is supported.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
#[cfg(trace_channel)] 
#[macro_use] extern crate debugit;
extern crate spin;
extern crate irq_safety;
extern crate wait_queue;
extern crate task;
extern crate scheduler;

#[cfg(downtime_eval)]
extern crate hpet;

use core::fmt;
use alloc::sync::Arc;
use irq_safety::MutexIrqSafe;
use spin::Mutex;
use wait_queue::{WaitQueue, WaitGuard, WaitError};


/// A wrapper type for an `ExchangeSlot` that is used for sending only.
struct SenderSlot<T>(Arc<MutexIrqSafe<ExchangeState<T>>>);
/// A wrapper type for an `ExchangeSlot` that is used for receiving only.
struct ReceiverSlot<T>(Arc<MutexIrqSafe<ExchangeState<T>>>);


/// An `ExchangeSlot` consists of two references to a shared state
/// that is used to exchange a message. 
/// 
/// There is a "sender" reference and a "receiver" reference, 
/// which are wrapped in their respective types: `SenderSlot` and `ReceiverSlot`.
struct ExchangeSlot<T> {
    sender:   Mutex<Option<SenderSlot<T>>>,
    receiver: Mutex<Option<ReceiverSlot<T>>>,
}
impl<T> ExchangeSlot<T> {
    fn new() -> ExchangeSlot<T> {
        let inner = Arc::new(MutexIrqSafe::new(ExchangeState::Init));
        ExchangeSlot {
            sender: Mutex::new(Some(SenderSlot(inner.clone()))),
            receiver: Mutex::new(Some(ReceiverSlot(inner))),
        }
    }

    fn take_sender_slot(&self) -> Option<SenderSlot<T>> {
        self.sender.lock().take()
    }

    fn take_receiver_slot(&self) -> Option<ReceiverSlot<T>> {
        self.receiver.lock().take()
    }

    fn replace_sender_slot(&self, s: SenderSlot<T>) {
        let _old = self.sender.lock().replace(s);
        if _old.is_some() {
            error!("BUG: REPLACE SENDER SLOT WAS SOME ALREADY");
        }
    }

    fn replace_receiver_slot(&self, r: ReceiverSlot<T>) {
        let _old = self.receiver.lock().replace(r);
        if _old.is_some() {
            error!("BUG: REPLACE RECEIVER SLOT WAS SOME ALREADY");
        }
    }
}


/// The possible states of an exchange slot in a rendezvous channel.
/// TODO: we should improve this state machine using session types 
///       to check for valid state transitions at compile time.
enum ExchangeState<T> {
    /// Initial state: we're waiting for either a sender or a receiver.
    Init,
    /// A sender has arrived before a receiver. 
    /// The `WaitGuard` contains the blocked sender task,
    /// and the `T` is the message that will be exchanged.
    WaitingForReceiver(WaitGuard, T),
    /// A receiver has arrived before a sender.
    /// The `WaitGuard` contains the blocked receiver task.
    WaitingForSender(WaitGuard),
    /// Sender and Receiver have rendezvoused, and the receiver finished first.
    /// Thus, it is the sender's responsibility to reset to the initial state.
    ReceiverFinishedFirst,
    /// Sender and Receiver have rendezvoused, and the sender finished first.
    /// Thus, the message `T` is enclosed here for the receiver to take, 
    /// and it is the receivers's responsibility to reset to the initial state.
    SenderFinishedFirst(T),
}
impl<T> fmt::Debug for ExchangeState<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ExchangeState::{}", match self {
            ExchangeState::Init                     => "Init",
            ExchangeState::WaitingForReceiver(..)   => "WaitingForReceiver",
            ExchangeState::WaitingForSender(..)     => "WaitingForSender",
            ExchangeState::ReceiverFinishedFirst    => "ReceiverFinishedFirst",
            ExchangeState::SenderFinishedFirst(..)  => "SenderFinishedFirst",
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


/// Create a new channel that requires a sender a receiver to rendezvous
/// in order to exchange a message. 
/// 
/// Returns a tuple of `(Sender, Receiver)`.
pub fn new_channel<T: Send>() -> (Sender<T>, Receiver<T>) {
    let channel = Arc::new(Channel::<T> {
        slot: ExchangeSlot::new(),
        waiting_senders: WaitQueue::new(),
        waiting_receivers: WaitQueue::new(),
    });
    (
        Sender   { channel: channel.clone() },
        Receiver { channel }
    )
}



/// The inner channel for synchronous rendezvous-based communication
/// between `Sender`s and `Receiver`s. 
///
/// This struct contains one or more exchange slot(s) (`ExchangeSlot`) as well as
/// queues for tasks that waiting to send or receive messages via those exchange slots. 
///
/// Sender-side and Receiver-side references to an exchange slot can be obtained in both 
/// a blocking and non-blocking fashion, 
/// which supports both synchronous (rendezvous-based) and asynchronous channels.
struct Channel<T: Send> {
    /// In a zero-capacity synchronous channel, there is only a single slot,
    /// but senders and receivers perform a blocking wait on it until the slot becomes available.
    /// In contrast, a synchronous channel with a capacity of 1 would return a "channel full" error
    /// if the slot was taken, instead of blocking. 
    slot: ExchangeSlot<T>,
    waiting_senders: WaitQueue,
    waiting_receivers: WaitQueue,
}
impl<T: Send> Channel<T> {
    /// Obtain a sender slot, blocking until one is available.
    fn take_sender_slot(&self) -> Result<SenderSlot<T>, WaitError> {
        // Fast path: the uncontended case.
        if let Some(s) = self.try_take_sender_slot() {
            return Ok(s);
        }
        // Slow path: add ourselves to the waitqueue
        // trace!("waiting to acquire sender slot...");
        self.waiting_senders.wait_until(&|| self.try_take_sender_slot())
    }
    
    /// Obtain a receiver slot, blocking until one is available.
    fn take_receiver_slot(&self) -> Result<ReceiverSlot<T>, WaitError> {
        // Fast path: the uncontended case.
        if let Some(s) = self.try_take_receiver_slot() {
            return Ok(s);
        }
        // Slow path: add ourselves to the waitqueue
        // trace!("waiting to acquire receiver slot...");
        self.waiting_receivers.wait_until(&|| self.try_take_receiver_slot())
    }

    /// Try to obtain a sender slot in a non-blocking fashion,
    /// returning `None` if a slot is not immediately available.
    fn try_take_sender_slot(&self) -> Option<SenderSlot<T>> {
        self.slot.take_sender_slot()
    }

    /// Try to obtain a receiver slot in a non-blocking fashion,
    /// returning `None` if a slot is not immediately available.
    fn try_take_receiver_slot(&self) -> Option<ReceiverSlot<T>> {
        self.slot.take_receiver_slot()
    }
}


/// The sender (transmit) side of a channel.
#[derive(Clone)]
pub struct Sender<T: Send> {
    channel: Arc<Channel<T>>,
}
impl <T: Send> Sender<T> {
    /// Send a message, blocking until a receiver is ready.
    /// 
    /// Returns `Ok(())` if the message was sent and received successfully,
    /// otherwise returns an error. 
    pub fn send(&self, msg: T) -> Result<(), &'static str> {
        #[cfg(trace_channel)]
        trace!("rendezvous: sending msg: {:?}", debugit!(msg));

        #[cfg(downtime_eval)] {
            let value = hpet::get_hpet().as_ref().unwrap().get_counter();
            // debug!("Value {} {}", value, value % 1024);
            // Fault mimicing a memory write. Function could panic when getting task
            if (value % 4096) == 0  && task::with_current_task(|t| t.is_restartable()).unwrap_or(false) {
                // debug!("Fake error {}", value);
                unsafe { *(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555; }
            }
        }

        // obtain a sender-side exchange slot, blocking if necessary
        let sender_slot = self.channel.take_sender_slot().map_err(|_| "failed to take_sender_slot")?;

        // Here, either the sender (this task) arrived first and needs to wait for a receiver,
        // or a receiver has already arrived and is waiting for a sender. 
        let retval = {
            let mut exchange_state = sender_slot.0.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *exchange_state, ExchangeState::Init);
            match current_state {
                ExchangeState::Init => {
                    // Hold interrupts to avoid blocking & descheduling this task until we release the slot lock,
                    // which is currently done automatically because the slot uses a MutexIrqSafe.
                    let curr = task::get_my_current_task().ok_or("couldn't get current task")?;
                    *exchange_state = ExchangeState::WaitingForReceiver(
                        WaitGuard::new(curr).map_err(|_| "failed to create wait guard")?,
                        msg,
                    );
                    None
                }
                ExchangeState::WaitingForSender(receiver_to_notify) => {
                    // The message has been sent successfully. 
                    *exchange_state = ExchangeState::SenderFinishedFirst(msg);
                    // Notify the receiver task (outside of this match statement),
                    // but DO NOT restore the sender slot to the channel yet; 
                    // that will be done once the receiver is also finished with the slot (in SenderFinishedFirst).
                    Some(Ok(receiver_to_notify))
                }
                state => {
                    error!("BUG: Sender (at beginning) in invalid state {:?}", state);
                    *exchange_state = state;
                    Some(Err("BUG: Sender (at beginning) in invalid state"))
                }
            }
            // here, the sender slot lock is dropped
        };
        // In the above block, we handled advancing the state of the exchange slot. 
        // Now we need to handle other stuff (like notifying waiters) without holding the sender_slot lock.
        match retval {
            Some(Ok(receiver_to_notify)) => {
                drop(receiver_to_notify);
                return Ok(());
            }
            Some(Err(e)) => {
                // Restore the sender slot and notify waiting senders.
                self.channel.slot.replace_sender_slot(sender_slot);
                self.channel.waiting_senders.notify_one();
                return Err(e);
            }
            None => {
                scheduler::schedule();
            }
        }

        // Here, the sender (this task) is waiting for a receiver
        loop {
            {
                let exchange_state = sender_slot.0.lock();
                match &*exchange_state {
                    ExchangeState::WaitingForReceiver(blocked_sender, ..) => {
                        if task::with_current_task(|t| t != blocked_sender.task())
                            .unwrap_or(true)
                        {
                            return Err("BUG: CURR TASK WAS DIFFERENT THAN BLOCKED SENDER");
                        }
                        blocked_sender.block_again().map_err(|_| "failed to block sender")?;
                    }
                    _ => break,
                }
            }
            scheduler::schedule();
        }

        // Here, we are at the rendezvous point
        let retval = {
            let mut exchange_state = sender_slot.0.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *exchange_state, ExchangeState::Init);
            match current_state {
                ExchangeState::ReceiverFinishedFirst => {
                    // Ready to transfer another message.
                    *exchange_state = ExchangeState::Init; 
                    Ok(())
                }
                state => {
                    error!("BUG: Sender (while waiting) in invalid state {:?}", state);
                    *exchange_state = state;
                    Err("BUG: Sender (while waiting) in invalid state")
                }
            }
        };
        if retval.is_ok() {
            // Restore the receiver slot now that the receiver is finished, and notify waiting receivers.
            self.channel.slot.replace_receiver_slot(ReceiverSlot(sender_slot.0.clone()));
            self.channel.waiting_receivers.notify_one();
        }

        // Restore the sender slot and notify waiting senders.
        // trace!("sender done, restoring slot");
        self.channel.slot.replace_sender_slot(sender_slot);
        self.channel.waiting_senders.notify_one();
        // trace!("sender done, returning from send().");
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
                RendezvousState::Waiting(task_to_notify, dest) => {
                    *dest = Some(msg);
                    let _held_interrupts = irq_safety::hold_interrupts();
                    *task_to_notify = WaitGuard::new(curr_task.clone());
                    drop(task_to_notify); // notifies the receiver
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

/// The receiver side of a channel.
#[derive(Clone)]
pub struct Receiver<T: Send> {
    channel: Arc<Channel<T>>,
}
impl <T: Send> Receiver<T> {
    /// Receive a message, blocking until a sender is ready. 
    /// 
    /// Returns the message if it was received properly,
    /// otherwise returns an error.
    pub fn receive(&self) -> Result<T, &'static str> {
        // trace!("rendezvous: receive() entry");
        let curr_task = task::get_my_current_task().ok_or("couldn't get current task")?;
        
        // obtain a receiver-side exchange slot, blocking if necessary
        let receiver_slot = self.channel.take_receiver_slot().map_err(|_| "failed to take_receiver_slot")?;

        // Here, either the receiver (this task) arrived first and needs to wait for a sender,
        // or a sender has already arrived and is waiting for a receiver. 
        let retval = {
            let mut exchange_state = receiver_slot.0.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *exchange_state, ExchangeState::Init);
            match current_state {
                ExchangeState::Init => {
                    // Hold interrupts to avoid blocking & descheduling this task until we release the slot lock,
                    // which is currently done automatically because the slot uses a MutexIrqSafe.
                    *exchange_state = ExchangeState::WaitingForSender(WaitGuard::new(curr_task.clone()).map_err(|_| "failed to create wait guard")?);
                    None
                }
                ExchangeState::WaitingForReceiver(sender_to_notify, msg) => {
                    // The message has been received successfully! 
                    *exchange_state = ExchangeState::ReceiverFinishedFirst;
                    // Notify the sender task (outside of this match statement), 
                    // but DO NOT restore the receiver slot to the channel yet; 
                    // that will be done once the sender is also finished with the slot (in ReceiverFinishedFirst).
                    Some(Ok((sender_to_notify, msg)))
                }
                state => {
                    error!("BUG: Receiver (at beginning) in invalid state {:?}", state);
                    *exchange_state = state;
                    Some(Err("BUG: Receiver (at beginning) in invalid state"))
                }
            }
            // here, the receiver slot lock is dropped
        };
        // In the above block, we handled advancing the state of the exchange slot. 
        // Now we need to handle other stuff (like notifying waiters) without holding the receiver_slot lock.
        match retval {
            Some(Ok((sender_to_notify, msg))) => {
                drop(sender_to_notify);
                #[cfg(trace_channel)]
                trace!("rendezvous: received msg: {:?}", debugit!(msg));
                return Ok(msg);
            }
            Some(Err(e)) => {
                // Restore the receiver slot and notify waiting receivers.
                self.channel.slot.replace_receiver_slot(receiver_slot);
                self.channel.waiting_receivers.notify_one();
                return Err(e);
            }
            None => {
                scheduler::schedule();
            }
        }

        // Here, the receiver (this task) is waiting for a sender
        loop {
            {
                let exchange_state = receiver_slot.0.lock();
                match &*exchange_state {
                    ExchangeState::WaitingForSender(blocked_receiver) => {
                        warn!("spurious wakeup while receiver is WaitingForSender... re-blocking task.");
                        if blocked_receiver.task() != &curr_task {
                            return Err("BUG: CURR TASK WAS DIFFERENT THAN BLOCKED RECEIVER");
                        }
                        blocked_receiver.block_again().map_err(|_| "failed to block receiver")?;
                    }
                    _ => break,
                }
            }
            scheduler::schedule();
        }


        // Here, we are at the rendezvous point
        let retval = {
            let mut exchange_state = receiver_slot.0.lock();
            // Temporarily take ownership of the channel's waiting state so we can modify it;
            // the match statement below will advance the waiting state to the proper next state.
            let current_state = core::mem::replace(&mut *exchange_state, ExchangeState::Init);
            match current_state {
                ExchangeState::SenderFinishedFirst(msg) => {
                    // Ready to transfer another message.
                    *exchange_state = ExchangeState::Init; 
                    Ok(msg)
                }
                state => {
                    error!("BUG: Receiver (at end) in invalid state {:?}", state);
                    *exchange_state = state;
                    Err("BUG: Receiver (at end) in invalid state")
                }
            }
        };
        if retval.is_ok() {
            // Restore the sender slot now that the sender is finished, and notify waiting senders.
            self.channel.slot.replace_sender_slot(SenderSlot(receiver_slot.0.clone()));
            self.channel.waiting_senders.notify_one();
        }

        // Restore the receiver slot and notify waiting receivers.
        // trace!("receiver done, restoring slot");
        self.channel.slot.replace_receiver_slot(receiver_slot);
        self.channel.waiting_receivers.notify_one();
        // trace!("rendezvous: receiver done, returning from receive().");

        #[cfg(trace_channel)]
        trace!("rendezvous: received msg: {:?}", debugit!(retval));
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
