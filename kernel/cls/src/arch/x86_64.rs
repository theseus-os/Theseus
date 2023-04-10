use memory::VirtualAddress;
use x86_64::registers::model_specific::GsBase;

pub(crate) fn set_cls_register(address: VirtualAddress) {
    GsBase::write(x86_64::VirtAddr::new(address.value() as u64));
}
