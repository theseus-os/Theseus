use super::*;

use descriptors::Configuration;

pub fn init(config: Configuration, offset: usize) -> Result<usize, &'static str> {
    /*let shmem = self.alloc()?;
    loop {
        let frindex = self.op_regs()?.frame_index.read();
        let qh_active = shmem.queue_heads.get_by_addr(qh_addr)?.token.read().active();
        let qtd_active = shmem.transfer_descriptors.get(fqtd_index)?.token.read().active();
        let report = &shmem.common.pages.get(buf_index)?[..8];
        log::warn!("FRINDEX {}; QUEUE {}; TRANSFER {}; REPORT {:?}", frindex, qh_active, qtd_active, report);
        sleep_ms(10);
    }*/

    Ok(offset)
}