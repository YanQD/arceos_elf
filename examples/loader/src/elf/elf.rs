//! Relocate .rela.dyn sections
//! R_TYPE 与处理器架构有关，相关文档详见
//! x86_64: <https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/artifacts/master/raw/x86-64-ABI/abi.pdf?job=build>
use core::mem::size_of;

use alloc::{collections::btree_map::BTreeMap, vec::Vec};
use memory_addr::VirtAddr;
use xmas_elf::{program, sections::SectionData, symbol_table::Entry, ElfFile};
extern crate alloc;

use axhal::{mem::PAGE_SIZE_4K, paging::MappingFlags};
use axlog::{debug, info};

use crate::abi::lookup_abi_call;

use super::load::EXEC_ZONE_START;

const R_RISCV_32: u32 = 1;
const R_RISCV_64: u32 = 2;
const R_RISCV_RELATIVE: u32 = 3;
const R_JUMP_SLOT: u32 = 5;

const AT_PHDR: u8 = 3;
const AT_PHENT: u8 = 4;
const AT_PHNUM: u8 = 5;
const AT_PAGESZ: u8 = 6;

#[allow(unused)]
const AT_BASE: u8 = 7;
#[allow(unused)]
const AT_ENTRY: u8 = 9;
const AT_RANDOM: u8 = 25;

#[derive(Debug)]
#[allow(unused)]
/// To describe the relocation pair in the ELF
pub struct RelocatePair {
    /// the source address of the relocation
    pub src: VirtAddr,
    /// the destination address of the relocation
    pub dst: VirtAddr,
    /// the set of bits affected by this relocation
    pub count: usize,
}

/// The segment of the elf file, which is used to map the elf file to the memory space
#[allow(unused)]
pub struct ELFSegment {
    /// The start virtual address of the segment
    pub vaddr: VirtAddr,
    /// The size of the segment
    pub size: usize,
    /// The flags of the segment which is used to set the page table entry
    pub flags: MappingFlags,
    /// The data of the segment
    pub data: Option<Vec<u8>>,
}

/// To parse the elf file and return the segments of the elf file
///
/// # Arguments
///
/// * `elf_data` - The elf file data
/// * `elf_base_addr` - The base address of the elf file if the file will be loaded to the memory
///
/// # Return
/// Return the entry point
///
/// # Warning
/// It can't be used to parse the elf file which need the dynamic linker, but you can do this by calling this function recursively
#[allow(unused)]
pub fn get_elf_entry(elf: &ElfFile, _elf_base_addr: Option<usize>) -> VirtAddr {
    let elf_header = elf.header;
    let magic = elf_header.pt1.magic;
    if magic != [0x7f, 0x45, 0x4c, 0x46] {
        debug!("invalid elf!");
        return VirtAddr::from(0);
    }

    let entry = EXEC_ZONE_START + elf.header.pt2.entry_point() as usize;
    entry.into()
}

/// To parse the elf file and return the segments of the elf file
///
/// # Arguments
///
/// * `elf_data` - The elf file data
/// * `elf_base_addr` - The base address of the elf file if the file will be loaded to the memory
///
/// # Return
/// Return the entry point, the segments of the elf file and the relocate pairs
///
/// # Warning
/// It can't be used to parse the elf file which need the dynamic linker, but you can do this by calling this function recursively
#[allow(unused)]
pub fn get_elf_segments(elf: &ElfFile, _elf_base_addr: Option<usize>) -> Vec<ELFSegment> {
    let elf_header = elf.header;
    let magic = elf_header.pt1.magic;
    if magic != [0x7f, 0x45, 0x4c, 0x46] {
        debug!("invalid elf!");
        return Vec::new();
    }

    let mut segments = Vec::new();
    // Load Elf "LOAD" segments
    elf.program_iter()
        .filter(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
        .for_each(|ph| {
            let mut start_va = EXEC_ZONE_START + ph.virtual_addr() as usize;
            let end_va = EXEC_ZONE_START + (ph.virtual_addr() + ph.mem_size()) as usize;
            let mut start_offset = ph.offset() as usize;
            let end_offset = (ph.offset() + ph.file_size()) as usize;

            let front_pad = start_va % PAGE_SIZE_4K;
            start_va -= front_pad;
            start_offset -= front_pad;

            let mut flags = MappingFlags::empty();
            if ph.flags().is_read() {
                flags |= MappingFlags::READ;
            }
            if ph.flags().is_write() {
                flags |= MappingFlags::WRITE;
            }
            if ph.flags().is_execute() {
                flags |= MappingFlags::EXECUTE;
            }
            let data = Some(elf.input[start_offset..end_offset].to_vec());
            segments.push(ELFSegment {
                vaddr: VirtAddr::from(start_va),
                size: end_va - start_va,
                flags,
                data,
            });
        });

    segments
}

/// To parse the elf file and get the relocate pairs
///
/// # Arguments
///
/// * `elf` - The elf file
/// * `elf_base_addr` - The base address of the elf file if the file will be loaded to the memory
#[allow(unused)]
pub fn get_relocate_pairs(elf: &ElfFile, _elf_base_addr: Option<usize>) -> Vec<RelocatePair> {
    let mut pairs = Vec::new();
    
    // 处理 .rela.dyn 段
    if let Some(rela_dyn) = elf.find_section_by_name(".rela.dyn") {
        if let Ok(SectionData::Rela64(data)) = rela_dyn.get_data(elf) {
            if let Some(dyn_sym_table) = elf.find_section_by_name(".dynsym") {
                if let Ok(SectionData::DynSymbolTable64(dyn_sym_table)) = dyn_sym_table.get_data(elf) {
                    debug!("Relocating .rela.dyn");
                    for entry in data {
                        let dyn_sym = &dyn_sym_table[entry.get_symbol_table_index() as usize];
                        let destination = EXEC_ZONE_START + entry.get_offset() as usize;
                        let symbol_value = dyn_sym.value() as usize;
                        let addend = entry.get_addend() as usize;

                        match entry.get_type() {
                            R_RISCV_32 | R_RISCV_64 => {
                                if dyn_sym.shndx() == 0 {
                                    if let Ok(name) = dyn_sym.get_name(elf) {
                                        debug!(r#"Symbol "{}" not found"#, name);
                                        continue;
                                    }
                                }
                                pairs.push(RelocatePair {
                                    src: VirtAddr::from(symbol_value + addend),
                                    dst: VirtAddr::from(destination),
                                    count: if entry.get_type() == R_RISCV_32 { 4 } else { 8 },
                                });
                            }
                            R_RISCV_RELATIVE => {
                                pairs.push(RelocatePair {
                                    src: VirtAddr::from(EXEC_ZONE_START + addend),
                                    dst: VirtAddr::from(destination),
                                    count: size_of::<usize>(),
                                });
                            }
                            R_JUMP_SLOT => {
                                if let Ok(name) = dyn_sym.get_name(elf) {
                                    if let Some(func_addr) = lookup_abi_call(name) {
                                        pairs.push(RelocatePair {
                                            src: VirtAddr::from(func_addr),
                                            dst: VirtAddr::from(destination),
                                            count: size_of::<usize>(),
                                        });
                                    }
                                }
                            }
                            other => debug!("Unsupported relocation type: {}", other),
                        }
                    }
                }
            }
        }
    }

    // 处理 .rela.plt 段
    if let Some(rela_plt) = elf.find_section_by_name(".rela.plt") {
        if let Ok(SectionData::Rela64(data)) = rela_plt.get_data(elf) {
            if let Some(dyn_sym_table) = elf.find_section_by_name(".dynsym") {
                if let Ok(SectionData::DynSymbolTable64(dyn_sym_table)) = dyn_sym_table.get_data(elf) {
                    debug!("Relocating .rela.plt");
                    for entry in data {
                        let dyn_sym = &dyn_sym_table[entry.get_symbol_table_index() as usize];
                        let destination = EXEC_ZONE_START + entry.get_offset() as usize;
                        
                        if let Ok(name) = dyn_sym.get_name(elf) {
                            if let Some(func_addr) = lookup_abi_call(name) {
                                pairs.push(RelocatePair {
                                    src: VirtAddr::from(func_addr),
                                    dst: VirtAddr::from(destination),
                                    count: size_of::<usize>(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    debug!("Relocating done");
    pairs
}

/// To parse the elf file and get the auxv vectors
///
/// # Arguments
///
/// * `elf` - The elf file
/// * `elf_base_addr` - The base address of the elf file if the file will be loaded to the memory
#[allow(unused)]
pub fn get_auxv_vector(
    elf: &ElfFile,
    elf_base_addr: Option<usize>,
) -> BTreeMap<u8, usize> {
    // Some elf will load ELF Header (offset == 0) to vaddr 0. In that case, base_addr will be added to all the LOAD.
    let elf_header_vaddr: usize = if let Some(header) = elf
        .program_iter()
        .find(|ph| ph.get_type() == Ok(program::Type::Load))
    {
        // Loading ELF Header into memory.
        let vaddr = header.virtual_addr() as usize;

        if vaddr == 0 {
            if let Some(addr) = elf_base_addr {
                addr
            } else {
                panic!("ELF Header is loaded to vaddr 0, but no base_addr is provided");
            }
        } else {
            vaddr
        }
    } else {
        0
    };
    info!("ELF header addr: 0x{:x}", elf_header_vaddr);
    let mut map = BTreeMap::new();
    map.insert(
        AT_PHDR,
        elf_header_vaddr + elf.header.pt2.ph_offset() as usize,
    );
    map.insert(AT_PHENT, elf.header.pt2.ph_entry_size() as usize);
    map.insert(AT_PHNUM, elf.header.pt2.ph_count() as usize);
    map.insert(AT_RANDOM, 0);
    map.insert(AT_PAGESZ, PAGE_SIZE_4K);
    map
}
