use core::{
    slice::{from_raw_parts, from_raw_parts_mut},
    cmp::min,
};
use axlog::debug;
use xmas_elf::{program, symbol_table::Entry, ElfFile};

use crate::{abi::lookup_abi_call, elf::verify_elf_header};

use super::LoadError;

pub const PLASH_START: usize = 0xffff_ffc0_2200_0000;
pub const EXEC_ZONE_START: usize = 0xffff_ffc0_8010_0000;
const MAX_APP_SIZE: usize = 0x100000;

pub fn load_elf() -> u64 {
    debug!("Load payload ...");
    let elf_size = unsafe { *(PLASH_START as *const usize) };
    debug!("ELF size: 0x{:x}", elf_size);
    let elf_slice = unsafe { from_raw_parts((PLASH_START + 0x8) as *const u8, elf_size) };
    let elf = ElfFile::new(elf_slice).expect("Failed to parse ELF");
    
    // 检查 ELF 头
    verify_elf_header(&elf).expect("Invalid ELF header");

    let is_need_interp = { 
        elf.program_iter().any(|ph| {
            ph.get_type() == Ok(program::Type::Interp)
        })
    };

    debug!("Dynamic interpreter (.interp section) exists: {}", is_need_interp);

    let run_code =
        unsafe { from_raw_parts_mut(EXEC_ZONE_START as *mut u8, MAX_APP_SIZE) };

    let entry: u64 = {
        if !is_need_interp {
            // static and position independent executable
            let _ = load_exec(&elf, elf_slice, run_code);
            elf.header.pt2.entry_point()
        } else {
            load_dyn(&elf, elf_slice, run_code);
            EXEC_ZONE_START as u64 + elf.header.pt2.entry_point()
        }
    };
    
    debug!("Entry: 0x{:x}", entry);
    return entry;
}

fn load_exec(elf: &ElfFile, elf_slice: &[u8], run_code: &mut [u8]) -> Result<(), LoadError> {
    for ph in elf.program_iter() {
        if ph.get_type() != Ok(program::Type::Load) {
            debug!("skipping segment type: {:?}", ph.get_type());
            continue;
        }
        
        let offset = ph.offset() as usize;
        let filesz = ph.file_size() as usize;
        let memsz = ph.mem_size() as usize;
        let vaddr = ph.virtual_addr() as usize;
        let dest_addr = vaddr - EXEC_ZONE_START;
        
        debug!("Loading segment: offset=0x{:x}, filesz=0x{:x}, memsz=0x{:x}, vaddr=0x{:x}", 
            offset, filesz, memsz, vaddr);
        debug!("dest_addr: {}", dest_addr);
        
        // 复制段内容
        if filesz > 0 {
            let src: &[u8] = &elf_slice[offset..offset + filesz];
            let dest = &mut run_code[dest_addr..dest_addr + filesz];
            dest.copy_from_slice(src);
        }
        
        // 处理 .bss 等需要零初始化的部分
        if memsz > filesz {
            let dest = &mut run_code[dest_addr + filesz..dest_addr + memsz];
            dest.fill(0);
        }
    }

    if let Some(rela_section) = elf.find_section_by_name(".rela.dyn") {
        let rela_data = match rela_section.get_data(elf) {
            Ok(xmas_elf::sections::SectionData::Rela64(data)) => data,
            _ => return Err(LoadError::RelocationError),
        };

        for rela in rela_data {
            debug!("Rela offset: 0x{:x}, type: {}, addend: 0x{:x}", 
                rela.get_offset(),
                rela.get_type(),
                rela.get_addend()
            );

            match rela.get_type() {
                3 => { // R_RISCV_RELATIVE
                    let new_value = rela.get_addend() as usize;
                    unsafe {
                        *(rela.get_offset() as *mut u64) = new_value as u64;
                    }
                },
                _ => {
                    debug!("Unsupported relocation type");
                }
            }
        }
    }
    
    Ok(())
}

fn load_dyn(elf: &ElfFile, elf_slice: &[u8], run_code: &mut [u8]) {
    for ph in elf.program_iter() {
        if ph.get_type() != Ok(program::Type::Load) {
            continue;
        }
        
        debug!("Loading segment: offset=0x{:x}, filesz=0x{:x}, memsz=0x{:x}, vaddr=0x{:x}", 
            ph.offset(), ph.file_size(), ph.mem_size(), ph.virtual_addr());
            
        load_segment(run_code, elf_slice, 
            ph.virtual_addr() as usize, 
            ph.offset() as usize, 
            ph.file_size() as usize, 
            ph.mem_size() as usize);
    }

    // 处理重定位
    modify_rela_dyn(elf);
    modify_rela_plt(elf);
}

fn load_segment(run_code: &mut [u8], elf_slice: &[u8], p_vaddr: usize, p_offset: usize, p_filesz: usize, p_memsz: usize) {
    let run_code_offset = p_vaddr;
    
    run_code[run_code_offset..run_code_offset + p_filesz]
        .copy_from_slice(&elf_slice[p_offset..p_offset + p_filesz]);
        
    if p_memsz > p_filesz {
        let zero_size = min(
            run_code.len() - p_filesz,
            p_memsz - p_filesz,
        );
        run_code[run_code_offset + p_filesz..run_code_offset + p_filesz + zero_size].fill(0);
    }
}

fn modify_rela_plt(elf: &ElfFile) {
    if let Some(rela_plt) = elf.find_section_by_name(".rela.plt") {
        let rela_data = match rela_plt.get_data(elf) {
            Ok(xmas_elf::sections::SectionData::Rela64(data)) => data,
            _ => return,
        };

        if let Some(dynsym) = elf.find_section_by_name(".dynsym") {
            let dynsym_data = match dynsym.get_data(elf) {
                Ok(xmas_elf::sections::SectionData::DynSymbolTable64(data)) => data,
                _ => return,
            };

            for rela in rela_data {
                let sym = &dynsym_data[rela.get_symbol_table_index() as usize];
                if let Ok(name) = sym.get_name(elf) {
                    debug!("Rela sym: {}", name);
                    if let Some(func_addr) = lookup_abi_call(name) {
                        debug!("func_addr 0x{:x}", func_addr);
                        unsafe {
                            *((EXEC_ZONE_START as u64 + rela.get_offset()) as *mut usize) = func_addr;
                        }
                    }
                }
            }
        }
    }
}

fn modify_rela_dyn(elf: &ElfFile) {
    if let Some(rela_section) = elf.find_section_by_name(".rela.dyn") {
        let rela_data = match rela_section.get_data(elf) {
            Ok(xmas_elf::sections::SectionData::Rela64(data)) => data,
            _ => return,
        };

        for rela in rela_data {
            debug!("Rela offset: 0x{:x}, type: {}, addend: 0x{:x}", 
                rela.get_offset(),
                rela.get_type(),
                rela.get_addend()
            );

            match rela.get_type() {
                3 => { // R_RISCV_RELATIVE
                    let reloc_addr = EXEC_ZONE_START + rela.get_offset() as usize;
                    let new_value = EXEC_ZONE_START + rela.get_addend() as usize;
                    unsafe {
                        *(reloc_addr as *mut u64) = new_value as u64;
                    }
                },
                _ => {
                    debug!("Unsupported relocation type");
                }
            }
        }
    }
}