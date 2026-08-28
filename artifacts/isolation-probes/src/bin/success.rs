use std::{fs::File, io::Write};

const RESPONSE: &str = r#"{"status":"ok","protocol_version":1,"changes":[{"path":"allowed","kind":"created","before_sha256":null,"after_sha256":"2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df"}],"warnings":[]}"#;

fn main() {
    let mut allowed = File::create("allowed").expect("workspace must allow creating allowed");
    allowed.write_all(b"ok").expect("allowed write must finish");
    drop(allowed);

    #[cfg(target_os = "linux")]
    if std::env::var_os("SPROUT_ISOLATION_NATIVE_SMOKE").as_deref() != Some("1".as_ref()) {
        linux_boundary::assert_exact_boundary();
    }

    print!("{RESPONSE}");
}

#[cfg(target_os = "linux")]
mod linux_boundary {
    use std::{env, ffi::c_void, fs::File, mem::size_of, os::raw::c_char};

    const F_GETFD: i32 = 1;
    const EBADF: i32 = 9;
    const EPERM: i32 = 1;
    const MS_REMOUNT: usize = 32;
    const AF_INET: i32 = 2;
    const SOCK_STREAM: i32 = 1;

    #[repr(C)]
    struct SockAddrIn {
        family: u16,
        port: u16,
        address: u32,
        zero: [u8; 8],
    }

    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
        fn __errno_location() -> *mut i32;
        fn mount(
            source: *const c_char,
            target: *const c_char,
            filesystem_type: *const c_char,
            flags: usize,
            data: *const c_void,
        ) -> i32;
        fn socket(domain: i32, socket_type: i32, protocol: i32) -> i32;
        fn connect(socket: i32, address: *const c_void, length: u32) -> i32;
        fn close(fd: i32) -> i32;
    }

    fn errno() -> i32 {
        // SAFETY: libc exposes one thread-local errno cell for the calling thread.
        unsafe { *__errno_location() }
    }

    pub(super) fn assert_exact_boundary() {
        assert!(File::create(".git/denied").is_err());
        assert!(File::create("/outside").is_err());
        for name in [
            "HOME",
            "AWS_ACCESS_KEY_ID",
            "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
            "AWS_CONTAINER_CREDENTIALS_FULL_URI",
            "SPROUT_ECS_PROOF_SECRET",
        ] {
            assert!(env::var_os(name).is_none(), "{name} leaked into the plugin");
        }
        assert_eq!(env::var("LANG").as_deref(), Ok("C"));
        assert!(File::open("/proc/self/environ").is_err());
        assert!(File::open("/etc/passwd").is_err());
        assert!(File::open("/run/secrets/credential").is_err());

        for fd in 3..1024 {
            // SAFETY: F_GETFD takes no variadic argument and does not mutate memory.
            assert_eq!(unsafe { fcntl(fd, F_GETFD) }, -1);
            assert_eq!(errno(), EBADF);
        }

        // SAFETY: all optional mount pointers are null and target is a static NUL-terminated path.
        let mount_result = unsafe {
            mount(
                std::ptr::null(),
                c"/workspace/.git".as_ptr(),
                std::ptr::null(),
                MS_REMOUNT,
                std::ptr::null(),
            )
        };
        assert_eq!(mount_result, -1);
        assert_eq!(errno(), EPERM);

        // 169.254.169.254:80 in the private network namespace must be unreachable.
        // SAFETY: socket arguments and the sockaddr pointer/length follow the Linux ABI.
        let socket_fd = unsafe { socket(AF_INET, SOCK_STREAM, 0) };
        assert!(socket_fd >= 0);
        let address = SockAddrIn {
            family: AF_INET as u16,
            port: 80_u16.to_be(),
            address: 0xA9FE_A9FE_u32.to_be(),
            zero: [0; 8],
        };
        // SAFETY: address remains valid for the complete synchronous call.
        assert_eq!(
            unsafe {
                connect(
                    socket_fd,
                    (&raw const address).cast(),
                    size_of::<SockAddrIn>() as u32,
                )
            },
            -1
        );
        // SAFETY: socket_fd is an owned live descriptor.
        assert_eq!(unsafe { close(socket_fd) }, 0);
    }
}
