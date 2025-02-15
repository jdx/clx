use clx::progress::Progress;

fn main() {
    let mut progress = Progress::new(100);
    for i in 0..100 {
        progress.update(i);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
