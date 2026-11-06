import ctypes, ctypes.util
x = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
x.XOpenDisplay.restype = ctypes.c_void_p
d = x.XOpenDisplay(None)
x.XDefaultRootWindow.restype = ctypes.c_ulong
r = x.XDefaultRootWindow(ctypes.c_void_p(d))
a = [ctypes.c_ulong() for _ in range(2)]; i = [ctypes.c_int() for _ in range(4)]; m = ctypes.c_uint()
x.XQueryPointer(ctypes.c_void_p(d), ctypes.c_ulong(r), ctypes.byref(a[0]), ctypes.byref(a[1]), *[ctypes.byref(v) for v in i], ctypes.byref(m))
print(f"pointer {i[0].value} {i[1].value}")
