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

use core::slice::from_raw_parts;

use alloc::string::ToString;
use axstd::println;
use elf::load::PLASH_START;
use process::Process;

#[unsafe(no_mangle)]
fn main() {
    println!("Load payload ...");
    let elf_size = unsafe { *(PLASH_START as *const usize) };
    
    println!("ELF size: 0x{:x}", elf_size);
    let elf_slice = unsafe { from_raw_parts((PLASH_START + 0x8) as *const u8, elf_size) };

    Process::init("fork".to_string(), &elf_slice).unwrap();

    println!("Process init done!");

}