(function() {var implementors = {};
implementors["io"] = [{"text":"impl&lt;IO&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"io/struct.ReaderWriter.html\" title=\"struct io::ReaderWriter\">ReaderWriter</a>&lt;IO&gt;","synthetic":false,"types":["io::ReaderWriter"]}];
implementors["memory"] = [{"text":"impl&lt;T:&nbsp;FromBytes&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"memory/struct.BorrowedMappedPages.html\" title=\"struct memory::BorrowedMappedPages\">BorrowedMappedPages</a>&lt;T, <a class=\"struct\" href=\"memory/struct.Mutable.html\" title=\"struct memory::Mutable\">Mutable</a>&gt;","synthetic":false,"types":["memory::paging::mapper::BorrowedMappedPages"]},{"text":"impl&lt;T:&nbsp;FromBytes&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"memory/struct.BorrowedSliceMappedPages.html\" title=\"struct memory::BorrowedSliceMappedPages\">BorrowedSliceMappedPages</a>&lt;T, <a class=\"struct\" href=\"memory/struct.Mutable.html\" title=\"struct memory::Mutable\">Mutable</a>&gt;","synthetic":false,"types":["memory::paging::mapper::BorrowedSliceMappedPages"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"memory/struct.PageTable.html\" title=\"struct memory::PageTable\">PageTable</a>","synthetic":false,"types":["memory::paging::PageTable"]}];
implementors["memory_structs"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"memory_structs/struct.PageRange.html\" title=\"struct memory_structs::PageRange\">PageRange</a>","synthetic":false,"types":["memory_structs::PageRange"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"memory_structs/struct.FrameRange.html\" title=\"struct memory_structs::FrameRange\">FrameRange</a>","synthetic":false,"types":["memory_structs::FrameRange"]}];
implementors["mutex_preemption"] = [{"text":"impl&lt;'a, T:&nbsp;?<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"mutex_preemption/struct.MutexPreemptGuard.html\" title=\"struct mutex_preemption::MutexPreemptGuard\">MutexPreemptGuard</a>&lt;'a, T&gt;","synthetic":false,"types":["mutex_preemption::mutex_preempt::MutexPreemptGuard"]},{"text":"impl&lt;'rwlock, T:&nbsp;?<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"mutex_preemption/struct.RwLockPreemptWriteGuard.html\" title=\"struct mutex_preemption::RwLockPreemptWriteGuard\">RwLockPreemptWriteGuard</a>&lt;'rwlock, T&gt;","synthetic":false,"types":["mutex_preemption::rwlock_preempt::RwLockPreemptWriteGuard"]}];
implementors["mutex_sleep"] = [{"text":"impl&lt;'a, T:&nbsp;?<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"mutex_sleep/struct.MutexSleepGuard.html\" title=\"struct mutex_sleep::MutexSleepGuard\">MutexSleepGuard</a>&lt;'a, T&gt;","synthetic":false,"types":["mutex_sleep::mutex::MutexSleepGuard"]},{"text":"impl&lt;'rwlock, T:&nbsp;?<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"mutex_sleep/struct.RwLockSleepWriteGuard.html\" title=\"struct mutex_sleep::RwLockSleepWriteGuard\">RwLockSleepWriteGuard</a>&lt;'rwlock, T&gt;","synthetic":false,"types":["mutex_sleep::rwlock::RwLockSleepWriteGuard"]}];
implementors["nic_buffers"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"nic_buffers/struct.TransmitBuffer.html\" title=\"struct nic_buffers::TransmitBuffer\">TransmitBuffer</a>","synthetic":false,"types":["nic_buffers::TransmitBuffer"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"nic_buffers/struct.ReceiveBuffer.html\" title=\"struct nic_buffers::ReceiveBuffer\">ReceiveBuffer</a>","synthetic":false,"types":["nic_buffers::ReceiveBuffer"]}];
implementors["no_drop"] = [{"text":"impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"no_drop/struct.NoDrop.html\" title=\"struct no_drop::NoDrop\">NoDrop</a>&lt;T&gt;","synthetic":false,"types":["no_drop::NoDrop"]}];
implementors["path"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"path/struct.Path.html\" title=\"struct path::Path\">Path</a>","synthetic":false,"types":["path::Path"]}];
implementors["pci"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"pci/struct.PciDevice.html\" title=\"struct pci::PciDevice\">PciDevice</a>","synthetic":false,"types":["pci::PciDevice"]}];
implementors["runqueue_priority"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"runqueue_priority/struct.PriorityTaskRef.html\" title=\"struct runqueue_priority::PriorityTaskRef\">PriorityTaskRef</a>","synthetic":false,"types":["runqueue_priority::PriorityTaskRef"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"runqueue_priority/struct.RunQueue.html\" title=\"struct runqueue_priority::RunQueue\">RunQueue</a>","synthetic":false,"types":["runqueue_priority::RunQueue"]}];
implementors["runqueue_realtime"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"runqueue_realtime/struct.RealtimeTaskRef.html\" title=\"struct runqueue_realtime::RealtimeTaskRef\">RealtimeTaskRef</a>","synthetic":false,"types":["runqueue_realtime::RealtimeTaskRef"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"runqueue_realtime/struct.RunQueue.html\" title=\"struct runqueue_realtime::RunQueue\">RunQueue</a>","synthetic":false,"types":["runqueue_realtime::RunQueue"]}];
implementors["runqueue_round_robin"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"runqueue_round_robin/struct.RoundRobinTaskRef.html\" title=\"struct runqueue_round_robin::RoundRobinTaskRef\">RoundRobinTaskRef</a>","synthetic":false,"types":["runqueue_round_robin::RoundRobinTaskRef"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"runqueue_round_robin/struct.RunQueue.html\" title=\"struct runqueue_round_robin::RunQueue\">RunQueue</a>","synthetic":false,"types":["runqueue_round_robin::RunQueue"]}];
implementors["serial_port"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"serial_port/struct.SerialPort.html\" title=\"struct serial_port::SerialPort\">SerialPort</a>","synthetic":false,"types":["serial_port::SerialPort"]}];
implementors["stack"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"stack/struct.Stack.html\" title=\"struct stack::Stack\">Stack</a>","synthetic":false,"types":["stack::Stack"]}];
implementors["text_terminal"] = [{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"text_terminal/struct.Line.html\" title=\"struct text_terminal::Line\">Line</a>","synthetic":false,"types":["text_terminal::Line"]},{"text":"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/ops/deref/trait.DerefMut.html\" title=\"trait core::ops::deref::DerefMut\">DerefMut</a> for <a class=\"struct\" href=\"text_terminal/struct.ScrollbackBuffer.html\" title=\"struct text_terminal::ScrollbackBuffer\">ScrollbackBuffer</a>","synthetic":false,"types":["text_terminal::ScrollbackBuffer"]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()