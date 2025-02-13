use axlog::{error, info};
use crate::process::current_process;

#[unsafe(no_mangle)]
pub extern "C" fn abi_fork() -> i32 {
    info!("[ABI:Process] Fork a new process!");
    
    // 1. 获取当前进程
    let curr_process = current_process();

    info!("Current process: {:?}", curr_process.is_zombie);

    // 2. 调用进程的 fork 方法
    match curr_process.fork() {
        Ok(child_pid) => {
            // fork成功
            // 父进程返回子进程pid
            // 子进程会在fork内部设置返回值为0
            child_pid as i32
        }
        Err(err) => {
            // fork失败返回错误码(负数)
            error!("[ABI:Process] Fork failed: {:?}", err);
            -1
        }
    }
}