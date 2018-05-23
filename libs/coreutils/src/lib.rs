#![no_std]
#![feature(alloc)]

extern crate rtc;
extern crate alloc;
#[macro_use] extern crate log;
use rtc::read_rtc;

use alloc::vec::Vec;
use alloc::string::String;

// Use once we figure out a system to register command functions to the terminal crate
// pub fn init() {
//     static func: fn() -> String = get_date();
//     console::add_command(String::from("date"), func);
// }






pub fn get_date() ->  String {
  
    let date = rtc::read_rtc();

    use alloc::string::ToString;
    let date_str = date.months.to_string() + &"/" +  &date.days.to_string() +
                                &"/" + &date.years.to_string() + " " + &date.hours.to_string() + ":" +  &date.minutes.to_string()
                                +":"+ &date.seconds.to_string() + &"\n";
    return date_str;
}

pub fn test() -> String {
    for _i in 1..5 {
        debug!("nice");
    for i in 1..200000{
        let _j = 1;
    }
    }
    // Ok(String::from("done\n"))
    return String::from("done\n");
}


// pub fn get_date(args: Vec<&'static str>) ->  Result<String, &'static str> {
  
//     let date = rtc::read_rtc();

//     use alloc::string::ToString;
//     let date_str = date.months.to_string() + &"/" +  &date.days.to_string() +
//                                 &"/" + &date.years.to_string() + " " + &date.hours.to_string() + ":" +  &date.minutes.to_string()
//                                 +":"+ &date.seconds.to_string() + &"\n";
//     Ok(date_str)
// }