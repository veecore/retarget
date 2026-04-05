use retarget::hook;

#[hook::objc::methods(class = "NSObject")]
impl MissingObjcHookMethods {
    unsafe extern "C" fn hash(
        this: *mut std::ffi::c_void,
        cmd: *mut std::ffi::c_void,
    ) -> usize {
        let _ = (this, cmd);
        0
    }
}

struct MissingObjcHookMethods;

fn main() {}
