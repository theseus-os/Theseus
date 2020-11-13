use alloc::string::String;

pub type TestResult = Result<(), String>;

pub struct Test<F> where F: Fn() -> TestResult {
    test: F,
    name: &'static str,
}

impl<F> Test<F> where F: Fn() -> TestResult {
    pub fn new(test: F, name: &'static str) -> Test<F> {
        Test {
            test,
            name,
        }
    }

    pub fn run(&self) -> Result<(), ()> {
        println!("Running test {}...", self.name);

        let result = match (self.test)() {
            Ok(_) => {
                println!("Test {} passed!", self.name);
                Ok(())
            },
            Err(msg) => {
                println!("Test {} failed: {}", self.name, msg);
                Err(())
            },
        };

        println!("");
        result
    }
}

macro_rules! test {
    ($f:path) => {
        ::test::Test::new($f, stringify!($f))
    };
}

macro_rules! testbed {
    ($($test:expr),+) => {
        {
            let mut passed = 0;
            let mut failed = 0;

            $(
                match $test.run() {
                    Ok(_) => {
                        passed += 1;
                    },
                    Err(_) => {
                        failed += 1;
                    }
                }
            )+

            println!("Total tests executed: {}", passed + failed);
            println!("Passed: {}    Failed: {}", passed, failed);

            if failed == 0 { 0 } else { 1 }
        }
    }
}
