//! Integration tests for COM hooks on Windows.

#[cfg(target_os = "windows")]
mod windows {
    use retarget::{hook, install_registered_hooks};
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{IStream, STGC_DEFAULT};
    use windows::core::{HRESULT, Interface};

    struct StreamHooks;

    #[hook::com_impl(interface = IStream, instance = test_stream_ptr())]
    impl StreamHooks {
        #[hook::com(field = SetSize)]
        unsafe extern "system" fn set_size(this: *mut c_void, libnewsize: u64) -> HRESULT {
            let _ = (this, libnewsize);
            HRESULT(0x80004005u32 as i32)
        }

        unsafe extern "system" fn commit(this: *mut c_void, grfcommitflags: u32) -> HRESULT {
            let _ = (this, grfcommitflags);
            HRESULT(0x80004005u32 as i32)
        }
    }

    #[test]
    fn intercepts_static_com_hooks() {
        let stream = test_stream();
        unsafe { stream.SetSize(16) }.expect("expected baseline IStream::SetSize to succeed");
        unsafe { stream.Commit(STGC_DEFAULT) }
            .expect("expected baseline IStream::Commit to succeed");

        install_registered_hooks().expect("expected Windows COM hooks to install");

        let set_size_error =
            unsafe { stream.SetSize(32) }.expect_err("expected detoured IStream::SetSize");
        assert_eq!(set_size_error.code(), HRESULT(0x80004005u32 as i32));

        let commit_error =
            unsafe { stream.Commit(STGC_DEFAULT) }.expect_err("expected detoured IStream::Commit");
        assert_eq!(commit_error.code(), HRESULT(0x80004005u32 as i32));
    }

    fn test_stream() -> IStream {
        let raw = test_stream_ptr();
        unsafe { <IStream as Interface>::from_raw_borrowed(&raw) }
            .expect("shared test stream pointer must stay valid")
            .clone()
    }

    fn test_stream_ptr() -> *mut c_void {
        *stream_slot() as *mut c_void
    }

    fn stream_slot() -> &'static usize {
        static STREAM: OnceLock<usize> = OnceLock::new();
        STREAM.get_or_init(|| {
            let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL(std::ptr::null_mut()), false) }
                .expect("expected CreateStreamOnHGlobal to succeed");
            let raw = stream.as_raw() as usize;
            std::mem::forget(stream);
            raw
        })
    }
}
