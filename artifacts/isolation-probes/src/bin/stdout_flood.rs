use std::{io::Write, thread, time::Duration};

fn main() {
    let mut stdout = std::io::stdout().lock();
    let block = [b'x'; 8192];
    for _ in 0..513 {
        stdout.write_all(&block).expect("stdout flood write failed");
    }
    stdout.flush().expect("stdout flood flush failed");

    #[cfg(feature = "smoke")]
    thread::sleep(Duration::from_millis(50));
    #[cfg(not(feature = "smoke"))]
    thread::sleep(Duration::from_secs(121));
}
