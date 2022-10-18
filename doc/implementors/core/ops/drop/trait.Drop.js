(function() {var implementors = {};
implementors["apic"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"apic/struct.LocalApic.html\" title=\"struct apic::LocalApic\">LocalApic</a>","synthetic":false,"types":["apic::LocalApic"]}];
implementors["async_channel"] = [{"text":"impl&lt;T:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"async_channel/struct.Receiver.html\" title=\"struct async_channel::Receiver\">Receiver</a>&lt;T&gt;","synthetic":false,"types":["async_channel::Receiver"]},{"text":"impl&lt;T:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"async_channel/struct.Sender.html\" title=\"struct async_channel::Sender\">Sender</a>&lt;T&gt;","synthetic":false,"types":["async_channel::Sender"]}];
implementors["atomic_linked_list"] = [{"text":"impl&lt;K, V&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"atomic_linked_list/atomic_map/struct.AtomicMap.html\" title=\"struct atomic_linked_list::atomic_map::AtomicMap\">AtomicMap</a>&lt;K, V&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;K: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a>,&nbsp;</span>","synthetic":false,"types":["atomic_linked_list::atomic_map::AtomicMap"]}];
implementors["crate_metadata"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"crate_metadata/struct.LoadedCrate.html\" title=\"struct crate_metadata::LoadedCrate\">LoadedCrate</a>","synthetic":false,"types":["crate_metadata::LoadedCrate"]}];
implementors["dfqueue"] = [{"text":"impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"dfqueue/mpsc_queue/struct.MpscQueue.html\" title=\"struct dfqueue::mpsc_queue::MpscQueue\">MpscQueue</a>&lt;T&gt;","synthetic":false,"types":["dfqueue::mpsc_queue::MpscQueue"]}];
implementors["frame_allocator"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"frame_allocator/struct.AllocatedFrames.html\" title=\"struct frame_allocator::AllocatedFrames\">AllocatedFrames</a>","synthetic":false,"types":["frame_allocator::AllocatedFrames"]},{"text":"impl&lt;'list&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"frame_allocator/struct.DeferredAllocAction.html\" title=\"struct frame_allocator::DeferredAllocAction\">DeferredAllocAction</a>&lt;'list&gt;","synthetic":false,"types":["frame_allocator::DeferredAllocAction"]}];
implementors["memory"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"memory/struct.MappedPages.html\" title=\"struct memory::MappedPages\">MappedPages</a>","synthetic":false,"types":["memory::paging::mapper::MappedPages"]}];
implementors["mod_mgmt"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"mod_mgmt/struct.AppCrateRef.html\" title=\"struct mod_mgmt::AppCrateRef\">AppCrateRef</a>","synthetic":false,"types":["mod_mgmt::AppCrateRef"]}];
implementors["mutex_sleep"] = [{"text":"impl&lt;'a, T:&nbsp;?<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"mutex_sleep/struct.MutexSleepGuard.html\" title=\"struct mutex_sleep::MutexSleepGuard\">MutexSleepGuard</a>&lt;'a, T&gt;","synthetic":false,"types":["mutex_sleep::mutex::MutexSleepGuard"]},{"text":"impl&lt;'rwlock, T:&nbsp;?<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"mutex_sleep/struct.RwLockSleepReadGuard.html\" title=\"struct mutex_sleep::RwLockSleepReadGuard\">RwLockSleepReadGuard</a>&lt;'rwlock, T&gt;","synthetic":false,"types":["mutex_sleep::rwlock::RwLockSleepReadGuard"]},{"text":"impl&lt;'rwlock, T:&nbsp;?<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"mutex_sleep/struct.RwLockSleepWriteGuard.html\" title=\"struct mutex_sleep::RwLockSleepWriteGuard\">RwLockSleepWriteGuard</a>&lt;'rwlock, T&gt;","synthetic":false,"types":["mutex_sleep::rwlock::RwLockSleepWriteGuard"]}];
implementors["nic_buffers"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"nic_buffers/struct.ReceiveBuffer.html\" title=\"struct nic_buffers::ReceiveBuffer\">ReceiveBuffer</a>","synthetic":false,"types":["nic_buffers::ReceiveBuffer"]}];
implementors["page_allocator"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"page_allocator/struct.AllocatedPages.html\" title=\"struct page_allocator::AllocatedPages\">AllocatedPages</a>","synthetic":false,"types":["page_allocator::AllocatedPages"]},{"text":"impl&lt;'list&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"page_allocator/struct.DeferredAllocAction.html\" title=\"struct page_allocator::DeferredAllocAction\">DeferredAllocAction</a>&lt;'list&gt;","synthetic":false,"types":["page_allocator::DeferredAllocAction"]}];
implementors["pmu_x86"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"pmu_x86/struct.Counter.html\" title=\"struct pmu_x86::Counter\">Counter</a>","synthetic":false,"types":["pmu_x86::Counter"]}];
implementors["preemption"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"preemption/struct.PreemptionGuard.html\" title=\"struct preemption::PreemptionGuard\">PreemptionGuard</a>","synthetic":false,"types":["preemption::PreemptionGuard"]}];
implementors["serial_port_basic"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"serial_port_basic/struct.SerialPort.html\" title=\"struct serial_port_basic::SerialPort\">SerialPort</a>","synthetic":false,"types":["serial_port_basic::SerialPort"]}];
implementors["spawn"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"spawn/struct.BootstrapTaskRef.html\" title=\"struct spawn::BootstrapTaskRef\">BootstrapTaskRef</a>","synthetic":false,"types":["spawn::BootstrapTaskRef"]}];
implementors["stdio"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"stdio/struct.KeyEventReadGuard.html\" title=\"struct stdio::KeyEventReadGuard\">KeyEventReadGuard</a>","synthetic":false,"types":["stdio::KeyEventReadGuard"]}];
implementors["task"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"task/struct.Task.html\" title=\"struct task::Task\">Task</a>","synthetic":false,"types":["task::Task"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"task/struct.JoinableTaskRef.html\" title=\"struct task::JoinableTaskRef\">JoinableTaskRef</a>","synthetic":false,"types":["task::JoinableTaskRef"]}];
implementors["text_terminal"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"enum\" href=\"text_terminal/enum.ScrollAction.html\" title=\"enum text_terminal::ScrollAction\">ScrollAction</a>","synthetic":false,"types":["text_terminal::ScrollAction"]}];
implementors["virtual_nic"] = [{"text":"impl&lt;S:&nbsp;<a class=\"trait\" href=\"nic_queues/trait.RxQueueRegisters.html\" title=\"trait nic_queues::RxQueueRegisters\">RxQueueRegisters</a>, T:&nbsp;<a class=\"trait\" href=\"intel_ethernet/descriptors/trait.RxDescriptor.html\" title=\"trait intel_ethernet::descriptors::RxDescriptor\">RxDescriptor</a>, U:&nbsp;<a class=\"trait\" href=\"nic_queues/trait.TxQueueRegisters.html\" title=\"trait nic_queues::TxQueueRegisters\">TxQueueRegisters</a>, V:&nbsp;<a class=\"trait\" href=\"intel_ethernet/descriptors/trait.TxDescriptor.html\" title=\"trait intel_ethernet::descriptors::TxDescriptor\">TxDescriptor</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"virtual_nic/struct.VirtualNic.html\" title=\"struct virtual_nic::VirtualNic\">VirtualNic</a>&lt;S, T, U, V&gt;","synthetic":false,"types":["virtual_nic::VirtualNic"]}];
implementors["wait_queue"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"wait_queue/struct.WaitGuard.html\" title=\"struct wait_queue::WaitGuard\">WaitGuard</a>","synthetic":false,"types":["wait_queue::WaitGuard"]}];
implementors["window"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/drop/trait.Drop.html\" title=\"trait core::ops::drop::Drop\">Drop</a> for <a class=\"struct\" href=\"window/struct.Window.html\" title=\"struct window::Window\">Window</a>","synthetic":false,"types":["window::Window"]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()