//! Process management syscalls
use crate::mm::{MapPermission, VirtAddr};
use crate::task::{
    change_program_brk, current_task_mmap, current_task_munmap, exit_current_and_run_next,
    get_syscall_info, read_buffer_from_va, suspend_current_and_run_next, write_buffer_to_va,
};
use crate::timer::get_time_us;
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    // 获取当前时间
    let us = get_time_us();
    let k_time = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let len = size_of::<TimeVal>();
    let buffer =
        unsafe { core::slice::from_raw_parts(&k_time as *const TimeVal as *const u8, len) };

    trace!("kernel: sys_get_time");
    if write_buffer_to_va(ts as *const TimeVal as usize, size_of::<TimeVal>(), buffer) {
        0
    } else {
        -1
    }
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");

    if trace_request == 2 {
        return get_syscall_info(id);
    }

    let mut read_buffer = [0u8; size_of::<usize>()];
    let read_buffer_slice = &mut read_buffer[..];

    if !read_buffer_from_va(id, size_of::<usize>(), read_buffer_slice) {
        return -1;
    }

    match trace_request {
        0 => {
            // read
            return read_buffer[0] as isize;
        }
        1 => {
            // write
            // write to user space id address
            let writer_buffer = [data as u8; 1];
            if !write_buffer_to_va(id, writer_buffer.len(), &writer_buffer) {
                return -1;
            }
            return 0;
        }
        _ => return -1,
    };
}

use crate::config::PAGE_SIZE;
// YOUR JOB: Implement mmap.
/// sys_mmap 仅对参数进行初步检查，并调用 current_task_mmap，具体内存映射逻辑在 current_task_mmap 中实现。
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    // permission

    // 错误检查: start没有按页大小对齐
    if start % PAGE_SIZE != 0 {
        return -1;
    }

    // 错误检查: port其余位必须为0
    if port & !0x7 != 0 {
        return -1;
    }

    // 错误检查4: port全为0(无意义的内存)
    if port & 0x7 == 0 {
        return -1;
    }

    let start_va = VirtAddr::from(start);
    // 检查start是否页对齐
    if !start_va.aligned() {
        error!("sys_mmap: start {:#x} not aligned", start);
        return -1;
    }
    let end_va = VirtAddr::from(start + len);

    let mut map_permission = MapPermission::U;
    if port & 0x1 != 0 {
        map_permission |= MapPermission::R;
    }
    if port & 0x2 != 0 {
        map_permission |= MapPermission::W;
    }
    if port & 0x4 != 0 {
        map_permission |= MapPermission::X;
    }

    current_task_mmap(start_va, end_va, map_permission)
}

use crate::log::error;
// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");

    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);

    if !start_va.aligned() {
        error!("sys_mmap: start {:#x} not aligned", start);
        return -1;
    }

    current_task_munmap(start_va, end_va)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
