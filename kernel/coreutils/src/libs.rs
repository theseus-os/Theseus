extern crate rtc;
use rtc::read_rtc;

pub fn date() {
    let dt = rtc::read_rtc();
    let date_str = date.months.to_string() + &"/" +  &date.days.to_string() +
                                &"/" + &date.years.to_string() + " " + &date.hours.to_string() + ":" +  &date.minutes.to_string()
                                +":"+ &date.seconds.to_string() + &"\n";
    try!(print_to_console(date_str));
}