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

pub fn return_ticks(){
    unsafe{
        trace!("{} ticks have passed ", TICKS);
        trace!("\n tick tick");
        trace!("\n tick tick");
        
    }   
    
}


struct time_keeping{
    start_time: u64,
    end_time: u64,

}