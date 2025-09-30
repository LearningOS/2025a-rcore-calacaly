//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::config::MAX_SYSCALL;
use crate::loader::{get_app_data, get_num_app};
use crate::mm::{translated_byte_buffer, MapPermission, VirtAddr};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::vec;
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
    /// syscall info
    syscall_info: Vec<[usize; MAX_SYSCALL]>,
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        let syscall_info = vec![[0_usize; MAX_SYSCALL]; num_app];
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                    syscall_info: syscall_info,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }
    fn get_syscall_info(&self, syscall_id: usize) -> isize {
        if syscall_id < MAX_SYSCALL {
            let inner = self.inner.exclusive_access();
            let current = inner.current_task;
            return inner.syscall_info[current][syscall_id] as isize;
        }
        -1
    }

    fn syscall_counts(&self, syscall_id: usize) {
        if syscall_id < MAX_SYSCALL {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.syscall_info[current][syscall_id] += 1;
        }
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// read data from user space to kernel space
pub fn read_buffer_from_va(va: usize, va_len: usize, read_buffer: &mut [u8]) -> bool {
    // 检测非法地址
    if va > ((1 << 39) - 1) {
        println!("read_buffer_from_va: va is illegal");
        return false;
    }

    let inner = TASK_MANAGER.inner.exclusive_access();
    let current = inner.current_task;
    let user_token = inner.tasks[current].get_user_token();

    let vpn = VirtAddr::from(va).floor();

    if let Some(pte) = inner.tasks[current].memory_set.translate(vpn) {
        if !pte.is_valid() || !pte.readable() {
            println!("pte is not readable or not valid");
            return false;
        }
    } else {
        println!("pte is not found");
        return false;
    }
    let read_len = read_buffer.len();
    let mut total_read = 0;
    let buffers = translated_byte_buffer(user_token, va as *const u8, va_len);
    for buffer in buffers {
        let to_read = core::cmp::min(buffer.len(), read_len - total_read);
        read_buffer[..to_read].copy_from_slice(&buffer[total_read..total_read + to_read]);
        total_read += to_read;
    }

    return total_read == va_len;
}

/// write data from kernel space to user space
pub fn write_buffer_to_va(va: usize, va_len: usize, write_buffer: &[u8]) -> bool {
    let inner = TASK_MANAGER.inner.exclusive_access();
    let current = inner.current_task;
    let user_token = inner.tasks[current].get_user_token();
    let vpn = VirtAddr::from(va).floor();
    if let Some(pte) = inner.tasks[current].memory_set.translate(vpn) {
        if !pte.is_valid() || !pte.writable() {
            println!("pte is not writable or not valid");
            return false;
        }
    } else {
        println!("pte is not exist");
        return false;
    }

    let write_len = write_buffer.len();
    let mut total_write = 0;
    let buffers = translated_byte_buffer(user_token, va as *const u8, va_len);
    for buffer in buffers {
        let to_write = core::cmp::min(buffer.len(), write_len - total_write);
        buffer[..to_write].copy_from_slice(&write_buffer[total_write..total_write + to_write]);
        total_write += to_write;
    }
    return total_write == va_len;
}

/// count syscall current task
pub fn syscall_counts(syscall_id: usize) {
    TASK_MANAGER.syscall_counts(syscall_id);
}

/// get syscall info current task
pub fn get_syscall_info(syscall_id: usize) -> isize {
    TASK_MANAGER.get_syscall_info(syscall_id)
}

/// mmap current task
pub fn current_task_mmap(start_va: VirtAddr, end_va: VirtAddr, permission: MapPermission) -> isize {
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let current = inner.current_task;

    inner.tasks[current]
        .memory_set
        .safe_insert_framed_area(start_va, end_va, permission)
}
/// unmap current task
pub fn current_task_munmap(start_va: VirtAddr, end_va: VirtAddr) -> isize {
    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let current = inner.current_task;
    inner.tasks[current]
        .memory_set
        .remove_framed_area(start_va, end_va)
}
