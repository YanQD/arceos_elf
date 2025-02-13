#![no_std]
#![no_main]
#![feature(c_variadic)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod abi;
mod config;
mod elf;
mod fs;
mod process;

use abi::abi_entry;
use axstd::println;
use elf::{load::load_elf, PLASH_START};

#[unsafe(no_mangle)]
fn main() {
    println!("Load payload ...");
    let elf_size = unsafe { *(PLASH_START as *const usize) };
    
    println!("ELF size: 0x{:x}", elf_size);

    let entry = load_elf();

    unsafe { 
        core::arch::asm!("
            la      a2, {abi_entry}
            mv      t2, {run_start}

            jalr    ra, t2, 0",
            abi_entry = sym abi_entry,
            run_start = in(reg) entry,
            clobber_abi("C"),
        )
    }

    println!("Process init done!");
}