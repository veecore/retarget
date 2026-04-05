use retarget::hook;

#[hook::objc::instance(class = "NSObject")]
unsafe extern "C" fn missing_selector() -> usize {
    0
}

fn main() {}
