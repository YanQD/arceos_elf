#![no_std]
#![no_main]
#![feature(c_variadic)]
#![feature(alloc_error_handler)]

extern crate alloc;

use abi::abi_entry;
use axlog::info;

mod abi;
mod config;
mod elf;
mod mem;
mod fs;
mod process;

use elf::load::load_elf;
use mem::MemorySet;
use process::load_app;

#[unsafe(no_mangle)]
fn main() {
    let entry = load_elf();

    let mut memory_set = MemorySet::new_memory_set();
    let _ = load_app(&mut memory_set);

    info!("Execute app ...");
    unsafe { core::arch::asm!("
        la      a2, {abi_entry}
        mv      t2, {run_start}
        jalr    t2",
        abi_entry = sym abi_entry,
        run_start = in(reg) entry,
        clobber_abi("C"),
    )}
}
