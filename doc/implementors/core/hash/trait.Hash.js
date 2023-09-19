(function() {var implementors = {
"apic":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"apic/struct.ApicId.html\" title=\"struct apic::ApicId\">ApicId</a>"]],
"ata":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"ata/struct.AtaStatus.html\" title=\"struct ata::AtaStatus\">AtaStatus</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"ata/struct.AtaError.html\" title=\"struct ata::AtaError\">AtaError</a>"]],
"boot_info":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"boot_info/struct.ElfSectionFlags.html\" title=\"struct boot_info::ElfSectionFlags\">ElfSectionFlags</a>"]],
"cpu":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"cpu/struct.CpuId.html\" title=\"struct cpu::CpuId\">CpuId</a>"]],
"crate_swap":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"crate_swap/struct.SwapRequest.html\" title=\"struct crate_swap::SwapRequest\">SwapRequest</a>"]],
"framebuffer":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"framebuffer/pixel/struct.RGBPixel.html\" title=\"struct framebuffer::pixel::RGBPixel\">RGBPixel</a>"],["impl&lt;P: <a class=\"trait\" href=\"framebuffer/pixel/trait.Pixel.html\" title=\"trait framebuffer::pixel::Pixel\">Pixel</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"framebuffer/struct.Framebuffer.html\" title=\"struct framebuffer::Framebuffer\">Framebuffer</a>&lt;P&gt;"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"framebuffer/pixel/struct.AlphaPixel.html\" title=\"struct framebuffer::pixel::AlphaPixel\">AlphaPixel</a>"]],
"keycodes_ascii":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"keycodes_ascii/struct.KeyboardModifiers.html\" title=\"struct keycodes_ascii::KeyboardModifiers\">KeyboardModifiers</a>"]],
"memory":[["impl&lt;T: FromBytes + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a>, M: <a class=\"trait\" href=\"memory/trait.Mutability.html\" title=\"trait memory::Mutability\">Mutability</a>, B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/borrow/trait.Borrow.html\" title=\"trait core::borrow::Borrow\">Borrow</a>&lt;<a class=\"struct\" href=\"memory/struct.MappedPages.html\" title=\"struct memory::MappedPages\">MappedPages</a>&gt;&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"memory/struct.BorrowedMappedPages.html\" title=\"struct memory::BorrowedMappedPages\">BorrowedMappedPages</a>&lt;T, M, B&gt;"],["impl&lt;T: FromBytes + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a>, M: <a class=\"trait\" href=\"memory/trait.Mutability.html\" title=\"trait memory::Mutability\">Mutability</a>, B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/borrow/trait.Borrow.html\" title=\"trait core::borrow::Borrow\">Borrow</a>&lt;<a class=\"struct\" href=\"memory/struct.MappedPages.html\" title=\"struct memory::MappedPages\">MappedPages</a>&gt;&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"memory/struct.BorrowedSliceMappedPages.html\" title=\"struct memory::BorrowedSliceMappedPages\">BorrowedSliceMappedPages</a>&lt;T, M, B&gt;"]],
"memory_structs":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"memory_structs/struct.VirtualAddress.html\" title=\"struct memory_structs::VirtualAddress\">VirtualAddress</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"memory_structs/struct.PhysicalAddress.html\" title=\"struct memory_structs::PhysicalAddress\">PhysicalAddress</a>"]],
"path":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"path/struct.Path.html\" title=\"struct path::Path\">Path</a>"]],
"pci":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"pci/struct.PciLocation.html\" title=\"struct pci::PciLocation\">PciLocation</a>"]],
"pte_flags":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"pte_flags/struct.PteFlags.html\" title=\"struct pte_flags::PteFlags\">PteFlags</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"pte_flags/struct.PteFlagsX86_64.html\" title=\"struct pte_flags::PteFlagsX86_64\">PteFlagsX86_64</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"pte_flags/struct.PteFlagsAarch64.html\" title=\"struct pte_flags::PteFlagsAarch64\">PteFlagsAarch64</a>"]],
"range_inclusive":[["impl&lt;Idx: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"range_inclusive/struct.RangeInclusive.html\" title=\"struct range_inclusive::RangeInclusive\">RangeInclusive</a>&lt;Idx&gt;"]],
"shapes":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"shapes/struct.Rectangle.html\" title=\"struct shapes::Rectangle\">Rectangle</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"shapes/struct.Coord.html\" title=\"struct shapes::Coord\">Coord</a>"]],
"signal_handler":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"enum\" href=\"signal_handler/enum.Signal.html\" title=\"enum signal_handler::Signal\">Signal</a>"]],
"str_ref":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"str_ref/struct.StrRef.html\" title=\"struct str_ref::StrRef\">StrRef</a>"]],
"sync_block":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"sync_block/struct.Block.html\" title=\"struct sync_block::Block\">Block</a>"]],
"sync_preemption":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"sync_preemption/struct.DisablePreemption.html\" title=\"struct sync_preemption::DisablePreemption\">DisablePreemption</a>"]],
"task":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"task/struct.TaskRef.html\" title=\"struct task::TaskRef\">TaskRef</a>"]],
"task_struct":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"task_struct/struct.Task.html\" title=\"struct task_struct::Task\">Task</a>"]],
"text_terminal":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"text_terminal/struct.FormatFlags.html\" title=\"struct text_terminal::FormatFlags\">FormatFlags</a>"]],
"time":[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"time/struct.Instant.html\" title=\"struct time::Instant\">Instant</a>"]]
};if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()