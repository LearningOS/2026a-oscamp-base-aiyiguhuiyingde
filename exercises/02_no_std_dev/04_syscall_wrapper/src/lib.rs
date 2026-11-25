//! # Cross-Architecture Syscall ABI Description and Wrapper
//!
//! Describe the syscall ABI for x86_64, aarch64, and riscv64 on Linux by filling in struct fields.
//! Also implement real syscall invocations on the current platform via conditional compilation.
//!
//! ## Background
//!
//! Different CPU architectures use different instructions and registers to trigger system calls:
//!
//! | Arch     | Instruction | Syscall ID Reg | Return Reg | Argument Registers              |
//! |----------|-------------|----------------|------------|---------------------------------|
//! | x86_64   | `syscall`   | rax            | rax        | rdi, rsi, rdx, r10, r8, r9     |
//! | aarch64  | `svc #0`    | x8             | x0         | x0, x1, x2, x3, x4, x5        |
//! | riscv64  | `ecall`     | a7             | a0         | a0, a1, a2, a3, a4, a5         |
//!
//! ## Task
//!
//! 1. Implement `x86_64_abi()`, `aarch64_abi()`, `riscv64_abi()` — return structs describing each arch's ABI
//! 2. (Conditional compilation) Implement real `syscall3` inline assembly on the current platform
//! 3. Build `sys_write` / `sys_read` / `sys_close` / `sys_exit` on top of `syscall3`
//!
//! ## Hints
//!
//! - Linux syscall numbers differ across architectures; x86_64 vs aarch64/riscv64 are quite different
//! - The x86_64 `syscall` instruction clobbers the rcx and r11 registers
//! - aarch64 and riscv64 share the unified syscall number table (from asm-generic)

#![cfg_attr(not(test), no_std)]

/// Describes a Linux Syscall ABI for a specific architecture
pub struct SyscallABI {
    /// Architecture name: "x86_64", "aarch64", "riscv64"
    pub arch: &'static str,
    /// Instruction that triggers the syscall: "syscall", "svc #0", "ecall"
    pub instruction: &'static str,
    /// Register holding the syscall number
    pub id_reg: &'static str,
    /// Register holding the return value
    pub ret_reg: &'static str,
    /// Argument registers (in order)
    pub arg_regs: &'static [&'static str],
    /// Registers additionally clobbered by the syscall instruction
    pub clobbered: &'static [&'static str],
    /// write syscall number
    pub sys_write: usize,
    /// read syscall number
    pub sys_read: usize,
    /// close syscall number
    pub sys_close: usize,
    /// exit syscall number
    pub sys_exit: usize,
}

/// Return the x86_64 Linux syscall ABI description
pub fn x86_64_abi() -> SyscallABI {
    SyscallABI {
        arch: "x86_64",
        instruction: "syscall",
        id_reg: "rax",
        ret_reg: "rax",
        arg_regs: &["rdi", "rsi", "rdx", "r10", "r8", "r9"],
        // `syscall` stashes the return address in rcx and copies RFLAGS into
        // r11 (and the kernel clobbers both), so they must be declared clobbered.
        clobbered: &["rcx", "r11"],
        sys_write: 1,
        sys_read: 0,
        sys_close: 3,
        sys_exit: 60,
    }
}

/// Return the aarch64 Linux syscall ABI description
pub fn aarch64_abi() -> SyscallABI {
    SyscallABI {
        arch: "aarch64",
        instruction: "svc #0",
        id_reg: "x8",
        ret_reg: "x0",
        arg_regs: &["x0", "x1", "x2", "x3", "x4", "x5"],
        // `svc` keeps everything else intact — no implicit clobbers.
        clobbered: &[],
        // asm-generic numbers, shared with riscv64.
        sys_write: 64,
        sys_read: 63,
        sys_close: 57,
        sys_exit: 93,
    }
}

/// Return the riscv64 Linux syscall ABI description
pub fn riscv64_abi() -> SyscallABI {
    SyscallABI {
        arch: "riscv64",
        instruction: "ecall",
        id_reg: "a7",
        ret_reg: "a0",
        arg_regs: &["a0", "a1", "a2", "a3", "a4", "a5"],
        clobbered: &[],
        // asm-generic numbers, shared with aarch64.
        sys_write: 64,
        sys_read: 63,
        sys_close: 57,
        sys_exit: 93,
    }
}

// ============================================================
// Real syscall implementation (conditional compilation, only active on matching platform)
// ============================================================

/// Issue a Linux syscall with up to 3 arguments.
///
/// # Safety
/// The caller must ensure the syscall number and arguments are valid.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub unsafe fn syscall3(id: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    let ret: isize;
    core::arch::asm!(
        "syscall",
        inlateout("rax") id => ret,
        in("rdi") arg0,
        in("rsi") arg1,
        in("rdx") arg2,
        // Destroyed by the instruction itself; must be listed so the compiler
        // does not keep live values in them across the call.
        lateout("rcx") _,
        lateout("r11") _,
        options(nostack)
    );
    ret
}

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub unsafe fn syscall3(id: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    let ret: isize;
    core::arch::asm!(
        "svc #0",
        in("x8") id,
        inlateout("x0") arg0 => ret,
        in("x1") arg1,
        in("x2") arg2,
        options(nostack)
    );
    ret
}

// Non-Linux platforms: provide a stub so the code compiles
#[cfg(all(not(target_os = "linux"), any(target_arch = "x86_64", target_arch = "aarch64")))]
pub unsafe fn syscall3(_id: usize, _arg0: usize, _arg1: usize, _arg2: usize) -> isize {
    panic!("syscall3 is only available on Linux")
}

/// riscv64 reaches the same wrapper through `ecall`; kept for completeness of
/// the cross-architecture story (and so a riscv64-linux build links).
#[cfg(all(target_arch = "riscv64", target_os = "linux"))]
pub unsafe fn syscall3(id: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    let ret: isize;
    core::arch::asm!(
        "ecall",
        in("a7") id,
        inlateout("a0") arg0 => ret,
        in("a1") arg1,
        in("a2") arg2,
        options(nostack)
    );
    ret
}

#[cfg(all(not(target_os = "linux"), not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
pub unsafe fn syscall3(_id: usize, _arg0: usize, _arg1: usize, _arg2: usize) -> isize {
    unimplemented!("no raw syscall path for this target")
}

// Platform-specific write syscall number
#[cfg(target_arch = "x86_64")]
const NATIVE_SYS_WRITE: usize = 1;
#[cfg(target_arch = "x86_64")]
const NATIVE_SYS_READ: usize = 0;
#[cfg(target_arch = "x86_64")]
const NATIVE_SYS_CLOSE: usize = 3;
#[cfg(target_arch = "x86_64")]
const NATIVE_SYS_EXIT: usize = 60;

#[cfg(target_arch = "aarch64")]
const NATIVE_SYS_WRITE: usize = 64;
#[cfg(target_arch = "aarch64")]
const NATIVE_SYS_READ: usize = 63;
#[cfg(target_arch = "aarch64")]
const NATIVE_SYS_CLOSE: usize = 57;
#[cfg(target_arch = "aarch64")]
const NATIVE_SYS_EXIT: usize = 93;

// Fallback for other architectures (not actually used, just for compilation)
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const NATIVE_SYS_WRITE: usize = 0;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const NATIVE_SYS_READ: usize = 0;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const NATIVE_SYS_CLOSE: usize = 0;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const NATIVE_SYS_EXIT: usize = 0;

/// Write data from `buf` to file descriptor `fd`.
///
/// On Linux the raw syscall is issued; elsewhere the same work is done through
/// the platform's C library so the wrapper stays usable.
#[cfg(target_os = "linux")]
pub fn sys_write(fd: usize, buf: &[u8]) -> isize {
    unsafe { syscall3(NATIVE_SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()) }
}

#[cfg(not(target_os = "linux"))]
pub fn sys_write(fd: usize, buf: &[u8]) -> isize {
    extern "C" {
        fn write(fd: core::ffi::c_int, buf: *const u8, count: usize) -> isize;
    }
    unsafe { write(fd as core::ffi::c_int, buf.as_ptr(), buf.len()) }
}

/// Read data from file descriptor `fd` into `buf`.
#[cfg(target_os = "linux")]
pub fn sys_read(fd: usize, buf: &mut [u8]) -> isize {
    unsafe { syscall3(NATIVE_SYS_READ, fd, buf.as_mut_ptr() as usize, buf.len()) }
}

#[cfg(not(target_os = "linux"))]
pub fn sys_read(fd: usize, buf: &mut [u8]) -> isize {
    extern "C" {
        fn read(fd: core::ffi::c_int, buf: *mut u8, count: usize) -> isize;
    }
    unsafe { read(fd as core::ffi::c_int, buf.as_mut_ptr(), buf.len()) }
}

/// Close file descriptor `fd`. Returns 0 on success, a negative errno otherwise.
#[cfg(target_os = "linux")]
pub fn sys_close(fd: usize) -> isize {
    unsafe { syscall3(NATIVE_SYS_CLOSE, fd, 0, 0) }
}

#[cfg(not(target_os = "linux"))]
pub fn sys_close(fd: usize) -> isize {
    extern "C" {
        fn close(fd: core::ffi::c_int) -> core::ffi::c_int;
        fn __errno_location() -> *mut core::ffi::c_int;
    }
    let rc = unsafe { close(fd as core::ffi::c_int) };
    if rc == 0 {
        0
    } else {
        // Mirror the Linux convention of returning -errno. `close()` reports
        // failures through errno rather than the return value on this libc.
        let e = unsafe { *__errno_location() };
        -(e as isize)
    }
}

/// Terminate the current process.
#[cfg(target_os = "linux")]
pub fn sys_exit(code: i32) -> ! {
    unsafe { syscall3(NATIVE_SYS_EXIT, code as usize, 0, 0) };
    loop {}
}

#[cfg(all(not(target_os = "linux"), target_env = "musl"))]
pub fn sys_exit(code: i32) -> ! {
    // musl (the Rust aarch64-apple-darwin std backend) exposes the Linux
    // syscall ABI through these symbols, so the real syscall path still works.
    extern "C" {
        fn syscall(number: core::ffi::c_long, ...) -> core::ffi::c_long;
    }
    unsafe { syscall(NATIVE_SYS_EXIT as core::ffi::c_long, code) };
    loop {}
}

/// glibc-style fallback for architectures whose libc does not forward the raw
/// syscall ABI to us. The number table above stays referenced via `let _`.
#[cfg(all(
    not(target_os = "linux"),
    not(target_env = "musl"),
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub fn sys_exit(code: i32) -> ! {
    extern "C" {
        fn _exit(status: core::ffi::c_int) -> !;
    }
    let _ = (
        NATIVE_SYS_WRITE,
        NATIVE_SYS_READ,
        NATIVE_SYS_CLOSE,
        NATIVE_SYS_EXIT,
    );
    unsafe { _exit(code) }
}

/// Bare `no_std` targets have no exit path available; this variant only exists
/// so such a build still links.
#[cfg(all(
    not(target_os = "linux"),
    not(target_env = "musl"),
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
pub fn sys_exit(_code: i32) -> ! {
    loop {}
}

// ============================================================
// Tests
// ============================================================
#[cfg(test)]
mod tests {
    use super::*;

    // ---- ABI knowledge tests (run on any platform) ----

    #[test]
    fn test_x86_64_instruction() {
        let abi = x86_64_abi();
        assert_eq!(abi.arch, "x86_64");
        assert_eq!(abi.instruction, "syscall");
    }

    #[test]
    fn test_x86_64_registers() {
        let abi = x86_64_abi();
        assert_eq!(abi.id_reg, "rax");
        assert_eq!(abi.ret_reg, "rax");
        assert_eq!(
            abi.arg_regs,
            &["rdi", "rsi", "rdx", "r10", "r8", "r9"],
            "x86_64 argument register order is incorrect"
        );
    }

    #[test]
    fn test_x86_64_clobbered() {
        let abi = x86_64_abi();
        assert!(
            abi.clobbered.contains(&"rcx") && abi.clobbered.contains(&"r11"),
            "x86_64 syscall clobbers rcx and r11"
        );
    }

    #[test]
    fn test_x86_64_syscall_numbers() {
        let abi = x86_64_abi();
        assert_eq!(abi.sys_write, 1);
        assert_eq!(abi.sys_read, 0);
        assert_eq!(abi.sys_close, 3);
        assert_eq!(abi.sys_exit, 60);
    }

    #[test]
    fn test_aarch64_instruction() {
        let abi = aarch64_abi();
        assert_eq!(abi.arch, "aarch64");
        assert_eq!(abi.instruction, "svc #0");
    }

    #[test]
    fn test_aarch64_registers() {
        let abi = aarch64_abi();
        assert_eq!(abi.id_reg, "x8");
        assert_eq!(abi.ret_reg, "x0");
        assert_eq!(
            abi.arg_regs,
            &["x0", "x1", "x2", "x3", "x4", "x5"],
            "aarch64 argument register order is incorrect"
        );
    }

    #[test]
    fn test_aarch64_clobbered() {
        let abi = aarch64_abi();
        assert!(
            abi.clobbered.is_empty(),
            "aarch64 svc does not clobber additional registers"
        );
    }

    #[test]
    fn test_aarch64_syscall_numbers() {
        let abi = aarch64_abi();
        assert_eq!(abi.sys_write, 64);
        assert_eq!(abi.sys_read, 63);
        assert_eq!(abi.sys_close, 57);
        assert_eq!(abi.sys_exit, 93);
    }

    #[test]
    fn test_riscv64_instruction() {
        let abi = riscv64_abi();
        assert_eq!(abi.arch, "riscv64");
        assert_eq!(abi.instruction, "ecall");
    }

    #[test]
    fn test_riscv64_registers() {
        let abi = riscv64_abi();
        assert_eq!(abi.id_reg, "a7");
        assert_eq!(abi.ret_reg, "a0");
        assert_eq!(
            abi.arg_regs,
            &["a0", "a1", "a2", "a3", "a4", "a5"],
            "riscv64 argument register order is incorrect"
        );
    }

    #[test]
    fn test_riscv64_clobbered() {
        let abi = riscv64_abi();
        assert!(
            abi.clobbered.is_empty(),
            "riscv64 ecall does not clobber additional registers"
        );
    }

    #[test]
    fn test_riscv64_syscall_numbers() {
        let abi = riscv64_abi();
        assert_eq!(abi.sys_write, 64);
        assert_eq!(abi.sys_read, 63);
        assert_eq!(abi.sys_close, 57);
        assert_eq!(abi.sys_exit, 93);
    }

    #[test]
    fn test_aarch64_riscv64_share_numbers() {
        let aarch64 = aarch64_abi();
        let riscv64 = riscv64_abi();
        assert_eq!(
            aarch64.sys_write, riscv64.sys_write,
            "aarch64 and riscv64 share asm-generic syscall numbers"
        );
        assert_eq!(aarch64.sys_read, riscv64.sys_read);
        assert_eq!(aarch64.sys_close, riscv64.sys_close);
        assert_eq!(aarch64.sys_exit, riscv64.sys_exit);
    }

    // ---- Real syscall tests (only run on Linux) ----

    #[cfg(target_os = "linux")]
    mod linux_tests {
        use super::*;

        #[test]
        fn test_sys_write_stdout() {
            let msg = b"[syscall_wrapper] sys_write test\n";
            let ret = sys_write(1, msg);
            assert_eq!(
                ret,
                msg.len() as isize,
                "sys_write should return bytes written"
            );
        }

        #[test]
        fn test_sys_write_stderr() {
            let msg = b"[syscall_wrapper] stderr test\n";
            let ret = sys_write(2, msg);
            assert_eq!(ret, msg.len() as isize);
        }

        #[test]
        fn test_sys_write_invalid_fd() {
            let ret = sys_write(999, b"hello");
            assert!(ret < 0, "invalid fd should return negative, got {ret}");
        }

        #[test]
        fn test_sys_close_invalid_fd() {
            let ret = sys_close(999);
            assert!(ret < 0, "closing invalid fd should return negative");
        }
    }
}
