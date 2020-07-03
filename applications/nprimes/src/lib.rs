#![no_std]
#![feature(core_intrinsics)]

extern crate alloc;
extern crate libm;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;

fn prime(n: u32, previous_primes: &Vec<u32>) -> bool{
    let mut prime = true;
    for prime_num in previous_primes {
        if (n > 9) && (prime_num * prime_num > n){
                break;
        }
        if n % prime_num == 0{
            prime = false;
            break;
        }
    }
    prime
}
    
pub fn main(args: Vec<String>) -> isize{
    if args.len() == 0{
        println!("You must provide a command line argument, exiting.");
        return 0;
    }

    //we have at least one command line argument, so we will use the first argument as the number of primes we would like to list
    let arg_string = args[0].clone();
    
    let mut num_primes : u32;
    
    if let Ok(i) = arg_string.parse::<u32>(){
        if i < 1{
            println!("You must input a number that is at least one. Exiting.");
            return 0;
        }
        else{
            num_primes = i;
        }
    }
    else{
        println!("You must input a valid integer argument. Exiting.");
        return 0;
    }

    println!("Calculating first {} primes:", num_primes);
    //creating an array to store the primes we have found so far
    let mut primes : Vec<u32> = Vec::new();
    primes.push(2);

    //now we will check increasing integers for prime status until we have num_primes of them
    let mut n = 3;
    while num_primes > 1{
        if prime(n, &primes){
            primes.push(n);
            num_primes -= 1;
        }
        n += 2;
    }

    //output our vector result
    println!("{:?}", primes);

    0
}