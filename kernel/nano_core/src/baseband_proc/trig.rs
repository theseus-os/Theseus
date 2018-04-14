// http://kineticmaths.com/index.php?title=Trigonometric_Ratios_in_the_Quadrants
// http://mathonweb.com/help_ebook/html/algorithms.htm#cos

pub fn pow(base: f32, exponent: i32) -> f32 {
    let mut result :f32 = 1.0;
    
    for i in 0..exponent {
        result = result*base;
    }

    return result;
}

//assuming input is in radians
pub fn sin(mut input: f32) -> f32 {
    let pi: f32 = 3.1415926;
    let mut sign : f32 = 1.0;

    //move angle to between 0 and 2*pi
    while !((0.0 <= input) && (input <= 2.0*pi)) {
        input = input - 2.0*pi;
    }

    //second quadrant
    if (pi/2.0 < input) && (input <= pi) {
        //move to 1st quadrant
        input = pi - input;
    }

    //third quadrant
    if (pi < input) && (input <= 3.0*pi/2.0) {
        sign = -1.0;
        //move to 1st quadrant
        input = input - pi;
    }

    //fourth quadrant
    if (3.0*pi/2.0 < input) && (input <= 2.0*pi) {
        sign = -1.0;
        //move to 1st quadrant
        input = 2.0*pi - input;
    }

    let output: f32;
    
    if input > pi/4.0 {
        output = cos(pi/2.0 - input);
    }
    else {
        output = input - ( pow(input,3) / 6.0 ) + ( pow(input,5) / 120.0 );
    }

    return output * sign
}


//assuming input is in radians
pub fn cos(mut input: f32) -> f32 {
    let pi: f32 = 3.1415926;
    let mut sign : f32 = 1.0;

    //move angle to between 0 and 2*pi
    while !((0.0 <= input) && (input <= 2.0*pi)) {
        input = input - 2.0*pi;
    }

    //second quadrant
    if (pi/2.0 < input) && (input <= pi) {
        sign = -1.0;
        //move to 1st quadrant
        input = pi - input;
    }

    //third quadrant
    if (pi < input) && (input <= 3.0*pi/2.0) {
        sign = -1.0;
        //move to 1st quadrant
        input = input - pi;
    }

    //fourth quadrant
    if (3.0*pi/2.0 < input) && (input <= 2.0*pi) {
        //move to 1st quadrant
        input = 2.0*pi - input;
    }

    let output: f32;
    
    if input > pi/4.0 {
        output = sin(pi/2.0 - input);
    }
    else {
        output = 1.0 - ( pow(input,2) / 2.0 ) + ( pow(input,4) / 24.0 ) - ( pow(input,6) / 720.0 );
    }

    return output * sign

}