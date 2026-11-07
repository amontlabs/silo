import ctypes
import errno
import os
import socket

libc = ctypes.CDLL(None, use_errno=True)
CLONE_NEWUSER = 0x10000000
syscalls = {'io_uring_setup': 425, 'io_uring_enter': 426, 'io_uring_register': 427, 'keyctl': 250,
            'add_key': 248, 'request_key': 249, 'bpf': 321, 'perf_event_open': 298, 'userfaultfd': 323,
            'clone3': 435, 'getpid(control)': 39}
for name, number in syscalls.items():
    ctypes.set_errno(0)
    result = libc.syscall(number, 0, 0, 0, 0, 0, 0)
    err = ctypes.get_errno()
    print(f'CHECK syscall.{name} result={result} errno={errno.errorcode.get(err, err) if result == -1 else "-"}')
ctypes.set_errno(0)
result = libc.unshare(CLONE_NEWUSER)
err = ctypes.get_errno()
print(f'CHECK unshare(CLONE_NEWUSER) result={result} errno={errno.errorcode.get(err, err) if result == -1 else "-"}')
for port in (3128, 9):
    s = socket.socket()
    s.settimeout(2)
    try:
        s.connect(('127.0.0.1', port))
        outcome = 'connected'
    except OSError as e:
        outcome = errno.errorcode.get(e.errno, str(e))
    print(f'CHECK tcp-connect-127.0.0.1:{port} {outcome}  (ECONNREFUSED = Landlock allowed the attempt; EACCES = Landlock denied it)')
    s.close()
