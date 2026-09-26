use std::time::Instant;
fn main() {
    let t = Instant::now();
    let r = ols_core::runtime::detect_system_install("redis");
    println!("redis {} ms {:?}", t.elapsed().as_millis(), r.map(|s| s.version));
}
