use sleep::sleep;

pub async fn main(_args: Vec<String>) -> isize {
    println!("Hello, future world!");
    sleep(1000).await;
    println!("Hello, past world!");
    0
}
