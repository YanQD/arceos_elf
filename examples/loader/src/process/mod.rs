//! This module provides the process management API for the operating system.

mod api;
mod elf;
mod stdio;
mod process;
mod fd_manager;
pub mod flags;

pub use api::*;
pub use process::{Process, PID2PC, TID2TASK};

#[cfg(target_arch = "x86_64")]
use axhal::arch::GdtStruct;
#[cfg(target_arch = "riscv64")]
use riscv::register::sstatus::{self, Sstatus};

use axhal::arch::TrapFrame;

#[cfg(target_arch = "riscv64")]
/// 用于第一次进入应用程序时的初始化
pub fn app_init_context(app_entry: usize, user_sp: usize) -> TrapFrame {
    let sstatus = sstatus::read();
    // 当前版本的riscv不支持使用set_spp函数，需要手动修改
    // 修改当前的sstatus为User，即是第8位置0
    let mut trap_frame = TrapFrame::default();
    trap_frame.regs.sp = user_sp;
    trap_frame.sepc = app_entry;
    trap_frame.sstatus =
        unsafe { (*(&sstatus as *const Sstatus as *const usize) & !(1 << 8)) & !(1 << 1) };
    unsafe {
        // a0为参数个数
        // a1存储的是用户栈底，即argv
        trap_frame.regs.a0 = *(user_sp as *const usize);
        trap_frame.regs.a1 = *(user_sp as *const usize).add(1);
    }
    trap_frame
}

#[cfg(target_arch = "x86_64")]
/// 用于第一次进入程序时的初始化
pub fn app_init_context(app_entry: usize, user_sp: usize) -> TrapFrame {
    TrapFrame {
        rip: app_entry as _,
        cs: GdtStruct::UCODE64_SELECTOR.0 as _,
        #[cfg(feature = "irq")]
        rflags: x86_64::registers::rflags::RFlags::INTERRUPT_FLAG.bits() as _,
        rsp: user_sp as _,
        ss: GdtStruct::UDATA_SELECTOR.0 as _,
        ..Default::default()
    }
}

#[cfg(target_arch = "aarch64")]
/// 用于第一次进入应用程序时的初始化
pub fn app_init_context(app_entry: usize, user_sp: usize) -> TrapFrame {
    let mut trap_frame = TrapFrame::default();
    trap_frame.usp = user_sp;
    trap_frame.elr = app_entry;
    trap_frame.spsr = 0x00000000;
    trap_frame
}

/// To write the trap frame into the kernel stack
///
/// # Safety
///
/// It should be guaranteed that the kstack address is valid and writable.
pub fn write_trapframe_to_kstack(kstack_top: usize, trap_frame: &TrapFrame) {
    let trap_frame_size = core::mem::size_of::<TrapFrame>();
    let trap_frame_ptr = (kstack_top - trap_frame_size) as *mut TrapFrame;
    unsafe {
        *trap_frame_ptr = trap_frame.clone();
    }
}

/// To read the trap frame from the kernel stack
///
/// # Safety
///
/// It should be guaranteed that the kstack address is valid and readable.
pub fn read_trapframe_from_kstack(kstack_top: usize) -> TrapFrame {
    let trap_frame_size = core::mem::size_of::<TrapFrame>();
    let trap_frame_ptr = (kstack_top - trap_frame_size) as *mut TrapFrame;
    unsafe { (*trap_frame_ptr).clone() }
}
