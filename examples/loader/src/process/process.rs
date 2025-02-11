//! 规定进程控制块内容
extern crate alloc;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use alloc::{collections::BTreeMap, string::String};
use axerrno::{AxError, AxResult};

use axhal::arch::write_page_table_root;
use axlog::info;
use axlog::debug;
use axmm::new_kernel_aspace;
use axmm::AddrSpace;
use axstd::println;
use axsync::Mutex;
use axtask::spawn_task;
use axtask::AxTaskRef;
use axtask::TaskId;
use axtask::TaskInner;
use memory_addr::PhysAddr;
use crate::abi::abi_entry;
use crate::config::KERNEL_PROCESS_ID;
use crate::config::TASK_STACK_SIZE;
use crate::fs::FileIO;
use crate::fs::OpenFlags;
use crate::process::load_user_app;
use crate::process::task_ext::TaskExt;
use crate::process::yield_now_task;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use crate::process::fd_manager::{FdManager, FdTable};

use crate::process::stdio::{Stderr, Stdin, Stdout};

/// Map from task id to arc pointer of task
pub static TID2TASK: Mutex<BTreeMap<u64, AxTaskRef>> = Mutex::new(BTreeMap::new());

/// Map from process id to arc pointer of process
pub static PID2PC: Mutex<BTreeMap<u64, Arc<Process>>> = Mutex::new(BTreeMap::new());

const FD_LIMIT_ORIGIN: usize = 1025;

#[allow(unused)]
/// The process control block
pub struct Process {
    /// 进程号
    pid: u64,
    /// 父进程号 
    pub parent: AtomicU64,
    /// 子进程
    pub children: Mutex<Vec<Arc<Process>>>,
    /// 所管理的线程
    pub tasks: Mutex<Vec<AxTaskRef>>,
    /// 文件描述符管理器
    pub fd_manager: FdManager,
    /// 进程状态
    pub is_zombie: AtomicBool,
    /// 退出状态码
    pub exit_code: AtomicI32,
    /// 地址空间
    pub memory_set: Mutex<Arc<Mutex<AddrSpace>>>,
    /// 堆空间管理
    pub heap_bottom: AtomicU64,
    pub heap_top: AtomicU64,
    /// 可执行文件路径
    pub file_path: Mutex<String>,
    /// the page table token of the process which the task belongs to
    pub page_table_token: AtomicU64,
}

impl Process {
    /// get the process id
    #[allow(unused)]
    pub fn pid(&self) -> u64 {
        self.pid
    }
    
    /// get the page table token
    #[allow(unused)]
    pub fn page_table_token(&self) -> u64 {
        self.page_table_token.load(Ordering::Acquire) 
    }
    /// get the parent process id
    #[allow(unused)]
    pub fn get_parent(&self) -> u64 {
        self.parent.load(Ordering::Acquire)
    }

    /// set the parent process id
    #[allow(unused)]
    pub fn set_parent(&self, parent: u64) {
        self.parent.store(parent, Ordering::Release)
    }

    /// get the exit code of the process
    #[allow(unused)]
    pub fn get_exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::Acquire)
    }
    
    /// set the exit code of the process
    #[allow(unused)]
    pub fn set_exit_code(&self, exit_code: i32) {
        self.exit_code.store(exit_code, Ordering::Release)
    }
    
    /// whether the process is a zombie process
    #[allow(unused)]
    pub fn get_zombie(&self) -> bool {
        self.is_zombie.load(Ordering::Acquire)
    }

    /// set the process as a zombie process
    #[allow(unused)]
    pub fn set_zombie(&self, status: bool) {
        self.is_zombie.store(status, Ordering::Release)
    }

    /// get the heap top of the process
    #[allow(unused)]
    pub fn get_heap_top(&self) -> u64 {
        self.heap_top.load(Ordering::Acquire)
    }

    /// set the heap top of the process
    #[allow(unused)]
    pub fn set_heap_top(&self, top: u64) {
        self.heap_top.store(top, Ordering::Release)
    }

    /// get the heap bottom of the process
    #[allow(unused)]
    pub fn get_heap_bottom(&self) -> u64 {
        self.heap_bottom.load(Ordering::Acquire)
    }

    /// set the heap bottom of the process
    #[allow(unused)]
    pub fn set_heap_bottom(&self, bottom: u64) {
        self.heap_bottom.store(bottom, Ordering::Release)
    }
    
    /// set the executable file path of the process
    #[allow(unused)]
    pub fn set_file_path(&self, path: String) {
        let mut file_path = self.file_path.lock();
        *file_path = path;
    }

    /// set the page table token of the process
    #[allow(unused)]
    pub fn set_page_table_token(&self, token: u64) {
        self.page_table_token.store(token, Ordering::Release);
    }
    
    /// get the executable file path of the process
    #[allow(unused)]
    pub fn get_file_path(&self) -> String {
        (*self.file_path.lock()).clone()
    }

    /// 若进程运行完成，则获取其返回码
    /// 若正在运行（可能上锁或没有上锁），则返回None
    #[allow(unused)]
    pub fn get_code_if_exit(&self) -> Option<i32> {
        if self.get_zombie() {
            return Some(self.get_exit_code());
        }
        None
    }
}

impl Process {
    /// 创建一个新的进程
    #[allow(unused)]
    pub fn new(
        pid: u64,
        parent: u64,
        memory_set: Mutex<Arc<Mutex<AddrSpace>>>,
        heap_bottom: u64,
        cwd: Arc<Mutex<String>>,
        mask: Arc<AtomicI32>,
        fd_table: FdTable,
    ) -> Self {
        let page_table_token = { 
            let ms = memory_set.lock();
            let token = ms.as_ref().lock().page_table_root().as_usize();
            AtomicU64::new(token as u64)
        };

        Self {
            pid,
            parent: AtomicU64::new(parent),
            children: Mutex::new(Vec::new()),
            tasks: Mutex::new(Vec::new()),
            is_zombie: AtomicBool::new(false),
            exit_code: AtomicI32::new(0),
            memory_set,
            heap_bottom: AtomicU64::new(heap_bottom),
            heap_top: AtomicU64::new(heap_bottom),
            fd_manager: FdManager::new(fd_table, cwd, mask, FD_LIMIT_ORIGIN),

            file_path: Mutex::new(String::new()),
            page_table_token,
        }
    }

    /// 根据给定参数创建一个新的进程
    #[allow(unused)]
    pub fn init(mut path: String, elf_file: &'static [u8]) -> AxResult<usize> {
        let mut memory_set = new_kernel_aspace().unwrap();

        let page_table_token = memory_set.page_table_root();

        info!("page_table_token: 0x{:x}", page_table_token);
        
        let (entry, heap_bottom) = load_user_app(&mut memory_set, "fork", elf_file).unwrap();
    
        let new_fd_table: FdTable = Arc::new(Mutex::new(vec![
            Some(Arc::new(Stdin { flags: Mutex::new(OpenFlags::empty()) })),
            Some(Arc::new(Stdout { flags: Mutex::new(OpenFlags::empty()) })),
            Some(Arc::new(Stderr { flags: Mutex::new(OpenFlags::empty()) })),
        ]));
    
        let new_process = Arc::new(Self::new(
            TaskId::new().as_u64(),
            KERNEL_PROCESS_ID,
            Mutex::new(Arc::new(Mutex::new(memory_set))),
            heap_bottom.as_usize() as u64,
            Arc::new(Mutex::new(String::from("/").into())),
            Arc::new(AtomicI32::new(0o022)),
            new_fd_table,
        ));
    
        if !path.starts_with('/') {
            let cwd = new_process.get_cwd();
            assert!(cwd.ends_with('/'));
            path = format!("{} {}", cwd, path); // 修复路径拼接
        }
    
        new_process.set_file_path(path.clone());
        
        let task_ext = TaskExt::init(new_process.pid(), true);

        let mut task_inner = TaskInner::new(
            move || {
                // 设置用户程序入口点
                unsafe { user_entry(entry.as_usize(), page_table_token); }
            },
            path.to_string(),
            TASK_STACK_SIZE,
        );

        task_inner.init_task_ext(task_ext);

        let new_task = spawn_task(task_inner);

        yield_now_task();

        TID2TASK.lock().insert(new_task.id().as_u64(), Arc::clone(&new_task));
        new_process.tasks.lock().push(Arc::clone(&new_task));
        PID2PC.lock().insert(new_process.pid(), Arc::clone(&new_process));

        Ok(page_table_token.as_usize())
    }

    pub fn fork() {
        
    }
}

pub unsafe extern "C" fn user_entry(entry: usize, page_table_token: PhysAddr) -> ! {
    unsafe { 
        write_page_table_root(page_table_token);
    }
    info!("entry: 0x{:x}", entry);
    println!("Jump to user space ...");

    unsafe { core::arch::asm!("
        la      a2, {abi_entry}
        mv      t2, {run_start}
        jalr    t2",
        abi_entry = sym abi_entry,
        run_start = in(reg) entry,
        clobber_abi("C"),
        options(noreturn),
    )}
}

/// 与文件相关的进程方法
impl Process {
    /// 为进程分配一个文件描述符
    #[allow(unused)]
    pub fn alloc_fd(&self, fd_table: &mut Vec<Option<Arc<dyn FileIO>>>) -> AxResult<usize> {
        for (i, fd) in fd_table.iter().enumerate() {
            if fd.is_none() {
                return Ok(i);
            }
        }
        if fd_table.len() >= self.fd_manager.get_limit() as usize {
            debug!("fd table is full");
            return Err(AxError::StorageFull);
        }
        fd_table.push(None);
        Ok(fd_table.len() - 1)
    }

    /// 获取当前进程的工作目录
    pub fn get_cwd(&self) -> String {
        self.fd_manager.cwd.lock().clone().to_string()
    }

    /// Set the current working directory of the process
    #[allow(unused)]
    pub fn set_cwd(&self, cwd: String) {
        *self.fd_manager.cwd.lock() = cwd.into();
    }
}
