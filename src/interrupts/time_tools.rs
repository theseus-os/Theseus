use interrupts::pit_clock::TICKS;




//adding PIC modules, may be unnecessary, will attempt to test and remove if that's the case


//initializing "tools" module, need to learn naming 


//do task with time specific to number input

/*
pub fn start_track(){

    main::start_tick = TICKS;

}
    
pub fn end_track(){

    let total_track = start_tick - TICKS;

    total_track;

}
*/


pub fn get_ticks() -> u64{
    
    //was used to test ticks were being received by this function
    /*
    unsafe{
    trace!("{} ticks have passed ", TICKS);
    }
    */

    unsafe{return TICKS;}
    
    
    
       
    

}

pub struct TimeKeeping{
    pub start_time: u64,
    pub end_time: u64,

}