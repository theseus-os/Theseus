use interrupts::pit_clock::TICKS;


//simply returns the tick count at the time it is called
pub fn get_ticks() -> u64{
    
    //was used to test ticks were being received by this function
    /*
    unsafe{
    trace!("{} ticks have passed ", TICKS);
    }
    */

    unsafe{ TICKS }
}

//could be used to test how many PIC ticks a function takes to complete, initiate with ticks at start, set end ticks after function, and subtract
//not properly tested: unsure what functions are running other than heartbeat function, which this can't be used on
pub struct TimeKeeping{
    pub start_time: u64,
    pub end_time: u64,

}