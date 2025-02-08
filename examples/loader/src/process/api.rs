use core::ptr::copy_nonoverlapping;
use core::slice::from_raw_parts;
extern crate alloc;
use alloc::sync::Arc;
use axerrno::{AxError, AxResult};
use axhal::mem::VirtAddr;
use axhal::paging::MappingFlags;
use axlog::{debug, info, warn};
use axtask::{current, AxTaskRef, CurrentTask, TaskExtRef};
use crate::config::{HEAP_BASE, MAX_HEAP_SIZE};
use crate::elf::elf::{get_elf_entry, get_elf_segments, get_relocate_pairs};
use crate::elf::load::{EXEC_ZONE_START, PLASH_START};
use crate::mem::MemorySet;

use super::{Process, PID2PC, TID2TASK};

// /// 初始化内核调度进程
// pub fn init_kernel_process() {
//     let kernel_process = Arc::new(Process::new(
//         TaskId::new().as_u64(),
//         TASK_STACK_SIZE as u64,
//         0,
//         Mutex::new(Arc::new(Mutex::new(MemorySet::new_empty()))),
//         0,
//         Arc::new(Mutex::new(String::from("/").into())),
//         Arc::new(AtomicI32::new(0o022)),
//         Arc::new(Mutex::new(vec![])),
//     ));

//     axtask::init_scheduler();
//     kernel_process
//         .tasks
//         .lock()
//         .push(Arc::clone(current_processor().idle_task()));
//     PID2PC.lock().insert(kernel_process.pid(), kernel_process);
// }

/// return the `Arc<Process>` of the current process
#[allow(unused)]
pub fn current_process() -> Arc<Process> {
    let current_task = current();

    let current_process = Arc::clone(PID2PC.lock().get(&current_task.task_ext().get_process_id()).unwrap());

    current_process
}

// /// 退出当前任务
// pub fn exit_current_task(exit_code: i32) -> ! {
//     let process = current_process();
//     let current_task = current();

//     let curr_id = current_task.id().as_u64();

//     info!("exit task id {} with code _{}_", curr_id, exit_code);

//     // clear_child_tid 的值不为 0，则将这个用户地址处的值写为0
//     let clear_child_tid = current_task.get_clear_child_tid();
//     if current_task.is_leader() {
//         loop {
//             let mut all_exited = true;
//             for task in process.tasks.lock().deref() {
//                 if !task.is_leader() && task.state() != TaskState::Exited {
//                     all_exited = false;
//                 }
//             }
//             if !all_exited {
//                 yield_now();
//             } else {
//                 break;
//             }
//         }
//         TID2TASK.lock().remove(&curr_id);
//         process.set_exit_code(exit_code);

//         process.set_zombie(true);

//         process.tasks.lock().clear();
//         process.fd_manager.fd_table.lock().clear();

//         let mut pid2pc = PID2PC.lock();
//         let kernel_process = pid2pc.get(&KERNEL_PROCESS_ID).unwrap();
//         // 将子进程交给idle进程
//         // process.memory_set = Arc::clone(&kernel_process.memory_set);
//         for child in process.children.lock().deref() {
//             child.set_parent(KERNEL_PROCESS_ID);
//             kernel_process.children.lock().push(Arc::clone(child));
//         }
//         pid2pc.remove(&process.pid());
//         drop(pid2pc);
//         drop(process);
//     } else {
//         TID2TASK.lock().remove(&curr_id);
//         // 从进程中删除当前线程
//         let mut tasks = process.tasks.lock();
//         let len = tasks.len();
//         for index in 0..len {
//             if tasks[index].id().as_u64() == curr_id {
//                 tasks.remove(index);
//                 break;
//             }
//         }
//         drop(tasks);

//         drop(process);
//     }
//     axtask::exit(exit_code);
// }

/// 返回 ELF 程序入口，堆底
pub fn load_app(
    memory_set: &mut MemorySet,
) -> AxResult<(VirtAddr, VirtAddr)> {
    debug!("Load payload ...");
    let elf_size = unsafe { *(PLASH_START as *const usize) };
    debug!("ELF size: 0x{:x}", elf_size);
    let elf_data = unsafe { from_raw_parts((PLASH_START + 0x8) as *const u8, elf_size) };

    let elf = xmas_elf::ElfFile::new(&elf_data).expect("Error parsing app ELF file.");
    let elf_base_addr = Some(EXEC_ZONE_START as usize);
    warn!("The elf base addr may be different in different arch!");
    let entry = get_elf_entry(&elf, elf_base_addr);
    let segments = get_elf_segments(&elf, elf_base_addr);
    let relocate_pairs = get_relocate_pairs(&elf, elf_base_addr);
    
    for segment in segments {
        memory_set.new_region(
            VirtAddr::from(segment.vaddr.as_usize()),
            segment.size,
            segment.flags,
            segment.data.as_deref(),
        );
    }

    for relocate_pair in relocate_pairs {
        let src: usize = relocate_pair.src.into();
        let dst: usize = relocate_pair.dst.into();
        let count = relocate_pair.count;
        unsafe { copy_nonoverlapping(src.to_ne_bytes().as_ptr(), dst as *mut u8, count) }
    }

    // Now map the stack and the heap
    let heap_start = VirtAddr::from(HEAP_BASE);
    let heap_data = [0_u8].repeat(MAX_HEAP_SIZE);
    memory_set.new_region(
        heap_start,
        MAX_HEAP_SIZE,
        MappingFlags::READ | MappingFlags::WRITE,
        Some(&heap_data),
    );

    info!(
        "[new region] user heap: [{:?}, {:?})",
        heap_start,
        heap_start + MAX_HEAP_SIZE
    );

    Ok((entry, heap_start))
}

// /// To deal with the page fault
// pub fn handle_page_fault(addr: VirtAddr, flags: MappingFlags) {
//     let current_process = current_process();
//     if current_process
//         .memory_set
//         .lock()
//         .lock()
//         .handle_page_fault(addr, flags)
//         .is_ok()
//     {
//         axhal::arch::flush_tlb(None);
//     }
// }

// /// 在当前进程找对应的子进程，并等待子进程结束
// /// 若找到了则返回对应的pid
// /// 否则返回一个状态
// ///
// /// # Safety
// ///
// /// 保证传入的 ptr 是有效的
// pub unsafe fn wait_pid(pid: i32, exit_code_ptr: *mut i32) -> Result<u64, WaitStatus> {
//     // 获取当前进程
//     let curr_process = current_process();
//     let mut exit_task_id: usize = 0;
//     let mut answer_id: u64 = 0;
//     let mut answer_status = WaitStatus::NotExist;
//     for (index, child) in curr_process.children.lock().iter().enumerate() {
//         if pid <= 0 {
//             if pid == 0 {
//                 axlog::warn!("Don't support for process group.");
//             }
//             // 任意一个进程结束都可以的
//             answer_status = WaitStatus::Running;
//             if let Some(exit_code) = child.get_code_if_exit() {
//                 answer_status = WaitStatus::Exited;
//                 info!("wait pid _{}_ with code _{}_", child.pid(), exit_code);
//                 exit_task_id = index;
//                 if !exit_code_ptr.is_null() {
//                     unsafe {
//                         // 因为没有切换页表，所以可以直接填写
//                         *exit_code_ptr = exit_code << 8;
//                     }
//                 }
//                 answer_id = child.pid();
//                 break;
//             }
//         } else if child.pid() == pid as u64 {
//             // 找到了对应的进程
//             if let Some(exit_code) = child.get_code_if_exit() {
//                 answer_status = WaitStatus::Exited;
//                 info!("wait pid _{}_ with code _{:?}_", child.pid(), exit_code);
//                 exit_task_id = index;
//                 if !exit_code_ptr.is_null() {
//                     unsafe {
//                         *exit_code_ptr = exit_code << 8;
//                         // 用于WEXITSTATUS设置编码
//                     }
//                 }
//                 answer_id = child.pid();
//             } else {
//                 answer_status = WaitStatus::Running;
//             }
//             break;
//         }
//     }
//     // 若进程成功结束，需要将其从父进程的children中删除
//     if answer_status == WaitStatus::Exited {
//         curr_process.children.lock().remove(exit_task_id);
//         return Ok(answer_id);
//     }
//     Err(answer_status)
// }

/// 以进程作为中转调用 task 的 yield
#[allow(unused)]
pub fn yield_now_task() {
    axtask::yield_now();
}

/// 以进程作为中转调用 task 的 sleep
#[allow(unused)]
pub fn sleep_now_task(dur: core::time::Duration) {
    axtask::sleep(dur);
}

/// current running task
#[allow(unused)]
pub fn current_task() -> CurrentTask {
    axtask::current()
}

/// 设置当前任务的 clear_child_tid
#[allow(unused)]
pub fn set_child_tid(tid: usize) {
    todo!()
}

/// Get the task reference by tid
#[allow(unused)]
pub fn get_task_ref(tid: u64) -> Option<AxTaskRef> {
    TID2TASK.lock().get(&tid).cloned()
}