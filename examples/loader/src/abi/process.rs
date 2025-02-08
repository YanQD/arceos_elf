use axlog::info;

#[unsafe(no_mangle)]
pub extern "C" fn abi_fork() -> i32 {
    info!("[ABI:Process] Fork a new process!");
    // // 获取当前进程
    // let current = Process::current();
    
    // // 创建新进程实例
    // let new_process = match Process::new() {
    //     Ok(p) => p,
    //     Err(_) => return -1,
    // };
    
    // // 设置父子关系
    // new_process.parent.store(current.pid(), Ordering::Release);
    // current.children.lock().push(Arc::new(new_process.clone()));
    
    // // 复制地址空间
    // let parent_ms = current.memory_set.lock();
    // let new_ms = match parent_ms.clone() {
    //     Ok(ms) => ms,
    //     Err(_) => return -1,
    // };
    // *new_process.memory_set.lock() = Arc::new(Mutex::new(new_ms));
    
    // // 复制文件描述符
    // if let Err(_) = new_process.fd_manager.clone_from(&current.fd_manager) {
    //     return -1;
    // }
    
    // // 复制其他必要的进程属性
    // new_process.heap_bottom.store(current.heap_bottom.load(Ordering::Acquire), Ordering::Release);
    // new_process.heap_top.store(current.heap_top.load(Ordering::Acquire), Ordering::Release);
    // new_process.stack_size.store(current.stack_size.load(Ordering::Acquire), Ordering::Release);
    // *new_process.file_path.lock() = current.file_path.lock().clone();
    
    // // 创建新的任务实例
    // let new_task = match Task::new() {
    //     Ok(t) => t,
    //     Err(_) => return -1,
    // };
    
    // // 设置任务上下文
    // // 注意：子进程从fork返回0，而父进程返回子进程的pid
    // let mut ctx = new_task.inner().ctx.get_mut();
    // *ctx = current_task().inner().ctx.get_mut().clone();
    // ctx.set_return_value(0); // 子进程返回0
    
    // // 将任务添加到进程中
    // new_process.tasks.lock().push(new_task.clone());
    
    // // 将新任务添加到调度器
    // scheduler::add_task(new_task);
    
    // // 父进程返回子进程的pid
    // new_process.pid() as i32
    todo!()
    // 0
}