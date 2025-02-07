use axlog::info;

#[unsafe(no_mangle)]
pub extern "C" fn abi_fork() -> i32 {
    info!("[ABI:Process] Fork a new process!");
    0
}
