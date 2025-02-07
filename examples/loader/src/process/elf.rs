//! Relocate .rela.dyn sections
//! R_TYPE 与处理器架构有关，相关文档详见
//! x86_64: <https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/artifacts/master/raw/x86-64-ABI/abi.pdf?job=build>
use core::{mem::size_of, ptr::null};

use alloc::{collections::btree_map::BTreeMap, string::String, vec::Vec};
use alloc::vec;
use memory_addr::VirtAddr;
use xmas_elf::symbol_table::Entry;
extern crate alloc;

use axhal::{mem::PAGE_SIZE_4K, paging::MappingFlags};
use axlog::info;

const R_RISCV_32: u32 = 1;
const R_RISCV_64: u32 = 2;
const R_RISCV_RELATIVE: u32 = 3;
const R_JUMP_SLOT: u32 = 5;
const TLS_DTPREL32: u32 = 8;
const TLS_DTV_OFFSET: usize = 0x800;

const AT_PHDR: u8 = 3;
const AT_PHENT: u8 = 4;
const AT_PHNUM: u8 = 5;
const AT_PAGESZ: u8 = 6;
#[allow(unused)]
const AT_BASE: u8 = 7;
#[allow(unused)]
const AT_ENTRY: u8 = 9;
const AT_RANDOM: u8 = 25;

pub const USER_INIT_STACK_SIZE: usize = 0x4000;

#[derive(Debug)]
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
pub fn get_elf_entry(elf: &xmas_elf::ElfFile, elf_base_addr: Option<usize>) -> VirtAddr {
    let elf_header = elf.header;
    let magic = elf_header.pt1.magic;
    assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");

    // Some elf will load ELF Header (offset == 0) to vaddr 0. In that case, base_addr will be added to all the LOAD.
    let base_addr = if let Some(header) = elf
        .program_iter()
        .find(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
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
            0
        }
    } else {
        0
    };
    info!("Base addr for the elf: 0x{:x}", base_addr);

    let entry = elf.header.pt2.entry_point() as usize + base_addr;
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
pub fn get_elf_segments(elf: &xmas_elf::ElfFile, elf_base_addr: Option<usize>) -> Vec<ELFSegment> {
    let elf_header = elf.header;
    let magic = elf_header.pt1.magic;
    assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");

    // Some elf will load ELF Header (offset == 0) to vaddr 0. In that case, base_addr will be added to all the LOAD.
    let base_addr = if let Some(header) = elf
        .program_iter()
        .find(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
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
            0
        }
    } else {
        0
    };
    info!("Base addr for the elf: 0x{:x}", base_addr);
    let mut segments = Vec::new();
    // Load Elf "LOAD" segments at base_addr.
    elf.program_iter()
        .filter(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
        .for_each(|ph| {
            let mut start_va = ph.virtual_addr() as usize + base_addr;
            let end_va = (ph.virtual_addr() + ph.mem_size()) as usize + base_addr;
            let mut start_offset = ph.offset() as usize;
            let end_offset = (ph.offset() + ph.file_size()) as usize;

            // Virtual address from elf may not be aligned.
            assert_eq!(start_va % PAGE_SIZE_4K, start_offset % PAGE_SIZE_4K);
            let front_pad = start_va % PAGE_SIZE_4K;
            start_va -= front_pad;
            start_offset -= front_pad;

            let mut flags = MappingFlags::USER;
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
pub fn get_relocate_pairs(
    elf: &xmas_elf::ElfFile,
    elf_base_addr: Option<usize>,
) -> Vec<RelocatePair> {
    let elf_header = elf.header;
    let magic = elf_header.pt1.magic;
    assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
    let mut pairs = Vec::new();
    // Some elf will load ELF Header (offset == 0) to vaddr 0. In that case, base_addr will be added to all the LOAD.
    let base_addr: usize = if let Some(header) = elf
        .program_iter()
        .find(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
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
            0
        }
    } else {
        0
    };
    info!("Base addr for the elf: 0x{:x}", base_addr);
    if let Some(rela_dyn) = elf.find_section_by_name(".rela.dyn") {
        let data = match rela_dyn.get_data(elf) {
            Ok(xmas_elf::sections::SectionData::Rela64(data)) => data,
            _ => panic!("Invalid data in .rela.dyn section"),
        };

        if let Some(dyn_sym_table) = elf.find_section_by_name(".dynsym") {
            let dyn_sym_table = match dyn_sym_table.get_data(elf) {
                Ok(xmas_elf::sections::SectionData::DynSymbolTable64(dyn_sym_table)) => {
                    dyn_sym_table
                }
                _ => panic!("Invalid data in .dynsym section"),
            };

            info!("Relocating .rela.dyn");
            for entry in data {
                let dyn_sym = &dyn_sym_table[entry.get_symbol_table_index() as usize];
                let destination = base_addr + entry.get_offset() as usize;
                let symbol_value = dyn_sym.value() as usize; // Represents the value of the symbol whose index resides in the relocation entry.
                let addend = entry.get_addend() as usize; // Represents the addend used to compute the value of the relocatable field.

                match entry.get_type() {
                    R_RISCV_32 => {
                        if dyn_sym.shndx() == 0 {
                            let name = dyn_sym.get_name(elf).unwrap();
                            panic!(r#"Symbol "{}" not found"#, name);
                        }
                        pairs.push(RelocatePair {
                            src: VirtAddr::from(symbol_value + addend),
                            dst: VirtAddr::from(destination),
                            count: 4,
                        })
                    }
                    R_RISCV_64 => {
                        if dyn_sym.shndx() == 0 {
                            let name = dyn_sym.get_name(elf).unwrap();
                            panic!(r#"Symbol "{}" not found"#, name);
                        }
                        pairs.push(RelocatePair {
                            src: VirtAddr::from(symbol_value + addend),
                            dst: VirtAddr::from(destination),
                            count: 8,
                        })
                    }
                    R_RISCV_RELATIVE => pairs.push(RelocatePair {
                        src: VirtAddr::from(base_addr + addend),
                        dst: VirtAddr::from(destination),
                        count: size_of::<usize>() / size_of::<u8>(),
                    }),
                    R_JUMP_SLOT => {
                        if dyn_sym.shndx() == 0 {
                            let name = dyn_sym.get_name(elf).unwrap();
                            panic!(r#"Symbol "{}" not found"#, name);
                        }
                        pairs.push(RelocatePair {
                            src: VirtAddr::from(symbol_value),
                            dst: VirtAddr::from(destination),
                            count: size_of::<usize>() / size_of::<u8>(),
                        })
                    }
                    TLS_DTPREL32 => pairs.push(RelocatePair {
                        src: VirtAddr::from(symbol_value + addend - TLS_DTV_OFFSET),
                        dst: VirtAddr::from(destination),
                        count: 4,
                    }),
                    other => panic!("Unknown relocation type: {}", other),
                }
            }
        }
    }

    // Relocate .rela.plt sections
    if let Some(rela_plt) = elf.find_section_by_name(".rela.plt") {
        let data = match rela_plt.get_data(elf) {
            Ok(xmas_elf::sections::SectionData::Rela64(data)) => data,
            _ => panic!("Invalid data in .rela.plt section"),
        };
        if elf.find_section_by_name(".dynsym").is_some() {
            let dyn_sym_table = match elf
                .find_section_by_name(".dynsym")
                .expect("Dynamic Symbol Table not found for .rela.plt section")
                .get_data(elf)
            {
                Ok(xmas_elf::sections::SectionData::DynSymbolTable64(dyn_sym_table)) => {
                    dyn_sym_table
                }
                _ => panic!("Invalid data in .dynsym section"),
            };

            info!("Relocating .rela.plt");
            for entry in data {
                let dyn_sym = &dyn_sym_table[entry.get_symbol_table_index() as usize];
                let destination = base_addr + entry.get_offset() as usize;
                match entry.get_type() {
                    R_JUMP_SLOT => {
                        let symbol_value = if dyn_sym.shndx() != 0 {
                            dyn_sym.value() as usize
                        } else {
                            let name = dyn_sym.get_name(elf).unwrap();
                            panic!(r#"Symbol "{}" not found"#, name);
                        }; // Represents the value of the symbol whose index resides in the relocation entry.
                        pairs.push(RelocatePair {
                            src: VirtAddr::from(symbol_value + base_addr),
                            dst: VirtAddr::from(destination),
                            count: size_of::<usize>(),
                        });
                    }
                    other => panic!("Unknown relocation type: {}", other),
                }
            }
        }
    }

    info!("Relocating done");
    pairs
}

/// To parse the elf file and get the auxv vectors
///
/// # Arguments
///
/// * `elf` - The elf file
/// * `elf_base_addr` - The base address of the elf file if the file will be loaded to the memory
pub fn get_auxv_vector(
    elf: &xmas_elf::ElfFile,
    elf_base_addr: Option<usize>,
) -> BTreeMap<u8, usize> {
    // Some elf will load ELF Header (offset == 0) to vaddr 0. In that case, base_addr will be added to all the LOAD.
    let elf_header_vaddr: usize = if let Some(header) = elf
        .program_iter()
        .find(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Load))
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
/// To get the app stack and the information on the stack from the ELF file
///
/// # Arguments
///
/// * `args` - The arguments of the app
/// * `envs` - The environment variables of the app
/// * `auxv` - The auxv vector of the app
/// * `stack_top` - The top address of the stack
/// * `stack_size` - The size of the stack.
///
/// # Return
///
/// `(stack_content, real_stack_bottom)`
///
/// * `stack_content`: the stack data from the low address to the high address, which will be used to map in the memory
///
/// * `real_stack_bottom`: The initial stack bottom is `stack_top + stack_size`.After push arguments into the stack, it will return the real stack bottom
///
/// The return data will be divided into two parts.
/// * The first part is the free stack content, which is all 0.
/// * The second part is the content carried by the user stack when it is initialized, such as args, auxv, etc.
///
/// The detailed format is described in <https://articles.manugarg.com/aboutelfauxiliaryvectors.html>
pub fn get_app_stack_region(
    args: Vec<String>,
    envs: &[String],
    auxv: BTreeMap<u8, usize>,
    stack_top: VirtAddr,
    stack_size: usize,
) -> (Vec<u8>, usize) {
    let ustack_top = stack_top;
    let ustack_bottom = ustack_top + stack_size;
    // The stack variable is actually the information carried by the stack
    let stack = init_stack(args, envs, auxv, ustack_bottom.into());
    let ustack_bottom = stack.get_sp();
    let mut data = [0_u8].repeat(stack_size - stack.get_len());
    data.extend(stack.get_data_front_ref());
    (data, ustack_bottom)
}

pub struct UserStack {
    /// 当前的用户栈的栈顶(低地址)
    sp: usize,
    /// 当前的用户栈的栈底(高地址)
    bottom: usize,
    /// data保存了用户栈上的信息
    pub data: Vec<u8>,
}

impl UserStack {
    pub fn new(sp: usize) -> Self {
        let data = vec![0; USER_INIT_STACK_SIZE];
        Self {
            sp,
            bottom: sp,
            data,
        }
    }
    pub fn get_data_front_ref(&self) -> &[u8] {
        let offset = self.data.len() - (self.bottom - self.sp);
        &self.data[offset..]
    }
    #[allow(unused)]
    pub fn get_data_front_mut_ref(&mut self) -> &mut [u8] {
        let offset = self.data.len() - (self.bottom - self.sp);
        &mut self.data[offset..]
    }
    /// 插入一段数据到用户栈中
    /// 返回的是插入后的用户栈顶，即这段数据的起始位置
    pub fn push<T: Copy>(&mut self, data: &[T]) {
        self.sp -= core::mem::size_of_val(data);
        self.sp -= self.sp % align_of::<T>();
        let offset = self.data.len() - (self.bottom - self.sp);
        unsafe {
            core::slice::from_raw_parts_mut(
                self.data.as_mut_ptr().add(offset) as *mut T,
                data.len(),
            )
        }
        .copy_from_slice(data);
    }
    /// 记得插入后补0
    pub fn push_str(&mut self, str: &str) -> usize {
        self.push(&[b'\0']);
        self.push(str.as_bytes());
        self.sp
    }
    pub fn get_sp(&self) -> usize {
        self.sp
    }
    // 获取真实的栈占用的内容
    pub fn get_len(&self) -> usize {
        self.bottom - self.sp
    }
}

/// 初始化用户栈
pub fn init_stack(
    args: Vec<String>,
    envs: &[String],
    auxv: BTreeMap<u8, usize>,
    sp: usize,
) -> UserStack {
    let mut stack = UserStack::new(sp);
    let random_str: &[usize; 2] = &[3703830112808742751usize, 7081108068768079778usize];
    stack.push(random_str.as_slice());
    let random_str_pos = stack.get_sp();
    // 按照栈的结构，先加入envs和argv的对应实际内容
    let envs_slice: Vec<_> = envs
        .iter()
        .map(|env| stack.push_str(env.as_str()))
        .collect();
    let argv_slice: Vec<_> = args
        .iter()
        .map(|arg| stack.push_str(arg.as_str()))
        .collect();
    // 加入envs和argv的地址
    stack.push(&[null::<u8>(), null::<u8>()]);
    let final_sp = stack.get_sp()
        - (auxv.len() * 2 + envs_slice.len() + argv_slice.len()) * core::mem::size_of::<usize>()
        - 8 // auxv 与 envs 之间的空位
        - 8 // envs 与 args 之间的空位
        - 8; // argc 占用空间
    if final_sp % 16 != 0 {
        // 按照 SIMD 要求，保证最终用户栈是 16 Bytes 对齐的
        // 更高的对齐要求理应对其他环境也适用，因此这里没有特殊指定 feature("fp_simd")
        stack.push(&[null::<u8>()]);
    }
    // 再加入auxv
    // 注意若是atrandom，则要指向栈上的一个16字节长度的随机字符串
    for (key, value) in auxv.iter() {
        if (*key) == 25 {
            // AT RANDOM
            stack.push(&[*key as usize, random_str_pos]);
        } else {
            stack.push(&[*key as usize, *value]);
        }
    }
    // 加入envs和argv的地址
    stack.push(&[null::<u8>()]);
    stack.push(envs_slice.as_slice());
    stack.push(&[null::<u8>()]);
    stack.push(argv_slice.as_slice());
    // 加入argc
    stack.push(&[args.len()]);
    stack
}
