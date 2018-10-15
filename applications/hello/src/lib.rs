#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;

use alloc::{Vec, String};


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    println!("Hello, world! Args: {:?}", _args);
    println!("Ecotourism has grown tremendously in recent years due to a widely advertised feature over conventional tourism: ecotourists can positively impact the environment and empower local people while simultaneously enjoying the same luxuries of conventional tourism. However, James Carrier and Donald Macleod suggest that ecotourism and its implications may not be as positive and straightforward as branded by the travel companies which sell it. Bursting the Bubble: The Socio-Cultural Context of Ecotourism explores the inappropriately broad definition of ecotourism that allows tourist facilities to exploit the appeal of ecotourism without actually delivering on its promise and the ecotourist bubble which shields these ecotourists from the consequences of creating and operating an ecotourism industry in a particular area. Close examination of these two aspects of ecotourism highlight the discrepancy between the stated intention of ecotourism and its more hidden and darker reality for the environment and local people. 
Bursting the Bubble examines two ecotourist locations in the Caribbean with histories of ecotourism that bear striking similarities: the village of Bayahibe in the Dominican Republic and Montego Bay in Jamaica. Both locations relied heavily on agricultural exports before marketing their natural attractions and subsequently developing ecotourism as their primary industry. Though ecotourism was created in these regions to empower the local people and preserve the environment, this industry has affected the environment, the people, and their relationship with it in mainly adverse ways. 
Firstly, the growth of the industry itself has necessitated expansion at the expense of the environment. For example, expansion of port facilities to handle more ships has led to extensive dredging and land fills (320). Expansion also requires more land, which entails displacement of local residents. Whole villages such as Bayahibe were relocated to provide the tourist industry with optimal coastal land, and local residents were often insufficiently compensated. Furthermore, ecotourism has also adversely affected other local industries, most notably the fishing one. Pollution from increased tourism reduces the yields of fishermen and creates tension between the fishing and tourism industry. Not only did ecotourism create tangible losses bore mostly by the citizens, it also created an socioeconomic divide between local residents and ecotourists, diminishing these residents to mere service labor and as just another tourist attraction. 
Independent ecotourists more interested in experiencing the region’s natural and cultural attractions often encountered difficulty in doing so, mainly because many of the parks were inclined to cater towards the large tour groups rather than individual travellers. Diving courses have been found to be a more successful way to attract these types of ecotourists. However, these ecotourists still had to travel to their destinations through the same pollutive means as other tourists, but the effects of this transportation are often forgotten, demonstrating the narrow extent of the ecotourist bubble. 
Ecotourism promotes efforts at environmental protection, but attempts at protecting the environment often result in effects just as negative as those from ecotourism itself. One of the more notable effects was the displacement of local residents to create the Del Este park in the Dominican Republic to preserve the environment; user fees charged by the park were not given in compensation to the displaced residents but rather sent directly to park management, thereby alienating residents from conservation efforts. This was seen most clearly when no local residents came to work at a beach clean-up event organized by the park management in 1999. 
A similar displacement occurred in Jamaica when fishermen were forced to fish in other areas after the creation of a marine park in the mid-1990s. In this case, however, the establishment of the park necessitated funding which the government did not adequately provide, forcing park management to rebrand the park as a commercial venue in order to garner revenue from ecotourists. In the end, the efforts of creating a marine park to preserve the environment resulted in an alienation of its people from their surroundings and the misconstruing of the park’s original intent as a conservation attempt. 
Ecotourism is often branded by travel agencies as a way to uplift local communities and the environment while still enjoying all the benefits of a conventional vacation. The vague meaning of ecotourism and the ecotourist bubble that shelters tourists from the negative implications of their actions only serve to further misalign this stated goal of ecotourism and its harsher reality and consequences. As this continues to happen, the noble idea of ecotourism will travel farther away from its actual implementation, and the distinction between ecotourism and conventional tourism will become harder to find. 
");

    0
}
