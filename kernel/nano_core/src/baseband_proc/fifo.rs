use alloc::Vec;

pub struct FIFO <T> {
    pub buffer: Vec<T>        
}

impl<T> FIFO <T> {
    pub fn init(&mut self) {
        self.buffer = Vec::new();
    }

    pub fn push(&mut self, packet: T){
        self.buffer.push(packet);
    }

    pub fn pop(&mut self) -> T {
        self.buffer.remove(0)
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

}