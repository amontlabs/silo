#!/usr/bin/env python3
"""Guest-side fixture for the desktop stream benchmark (development only).

Covers the X screen with one override-redirect window and grabs the keyboard.
`a` swaps the fill between a reddish and a bluish noise tile, so the viewer can
time a keypress until the changed picture is on screen. `m` starts or stops a
scrolling motion workload. `q` exits. Uses only libX11 through ctypes, so it
runs on any desktop guest without extra packages.
"""
import ctypes
import ctypes.util
import random
import select
import signal
import sys
import time

X = ctypes.CDLL(ctypes.util.find_library('X11') or 'libX11.so.6')
X.XOpenDisplay.restype = ctypes.c_void_p
X.XOpenDisplay.argtypes = [ctypes.c_char_p]
X.XDefaultScreen.argtypes = [ctypes.c_void_p]
X.XRootWindow.restype = ctypes.c_ulong
X.XRootWindow.argtypes = [ctypes.c_void_p, ctypes.c_int]
X.XDefaultVisual.restype = ctypes.c_void_p
X.XDefaultVisual.argtypes = [ctypes.c_void_p, ctypes.c_int]
X.XDefaultDepth.argtypes = [ctypes.c_void_p, ctypes.c_int]
X.XDefaultGC.restype = ctypes.c_void_p
X.XDefaultGC.argtypes = [ctypes.c_void_p, ctypes.c_int]
X.XCreatePixmap.restype = ctypes.c_ulong
X.XCreatePixmap.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
X.XCreateImage.restype = ctypes.c_void_p
X.XCreateImage.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_int,
                           ctypes.c_char_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_int]
X.XPutImage.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int,
                        ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint]
X.XCreateGC.restype = ctypes.c_void_p
X.XCreateGC.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_void_p]
X.XSetTile.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
X.XSetFillStyle.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int]
X.XSetTSOrigin.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
X.XFillRectangle.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_void_p, ctypes.c_int, ctypes.c_int,
                             ctypes.c_uint, ctypes.c_uint]
X.XMapRaised.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
X.XFlush.argtypes = [ctypes.c_void_p]
X.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
X.XPending.argtypes = [ctypes.c_void_p]
X.XNextEvent.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
X.XLookupKeysym.restype = ctypes.c_ulong
X.XLookupKeysym.argtypes = [ctypes.c_void_p, ctypes.c_int]
X.XConnectionNumber.argtypes = [ctypes.c_void_p]
X.XGrabKeyboard.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                            ctypes.c_ulong]
X.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
X.XDisplayWidth.argtypes = [ctypes.c_void_p, ctypes.c_int]
X.XDisplayHeight.argtypes = [ctypes.c_void_p, ctypes.c_int]


class XSetWindowAttributes(ctypes.Structure):
    _fields_ = [('background_pixmap', ctypes.c_ulong), ('background_pixel', ctypes.c_ulong),
                ('border_pixmap', ctypes.c_ulong), ('border_pixel', ctypes.c_ulong),
                ('bit_gravity', ctypes.c_int), ('win_gravity', ctypes.c_int),
                ('backing_store', ctypes.c_int), ('backing_planes', ctypes.c_ulong),
                ('backing_pixel', ctypes.c_ulong), ('save_under', ctypes.c_int),
                ('event_mask', ctypes.c_long), ('do_not_propagate_mask', ctypes.c_long),
                ('override_redirect', ctypes.c_int), ('colormap', ctypes.c_ulong),
                ('cursor', ctypes.c_ulong)]


X.XCreateWindow.restype = ctypes.c_ulong
X.XCreateWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_uint,
                            ctypes.c_uint, ctypes.c_uint, ctypes.c_int, ctypes.c_uint, ctypes.c_void_p,
                            ctypes.c_ulong, ctypes.POINTER(XSetWindowAttributes)]

KEY_PRESS, EXPOSE = 2, 12
KEY_PRESS_MASK, EXPOSURE_MASK = 1 << 0, 1 << 15
CW_BACK_PIXEL, CW_OVERRIDE_REDIRECT, CW_EVENT_MASK = 1 << 1, 1 << 9, 1 << 11
FILL_TILED, Z_PIXMAP = 1, 2
TILE = 256
EDGE = 4096


def noise_tile(display, root, gc, visual, depth, red):
    rng = random.Random(1 if red else 2)
    data = bytearray(TILE * TILE * 4)
    for i in range(0, len(data), 4):
        n = rng.randrange(80)
        # BGRX in memory on a little-endian TrueColor visual.
        data[i:i + 4] = bytes((40 + n, 40 + n, 175 + n, 0)) if red else bytes((175 + n, 40 + n, 40 + n, 0))
    buffer = ctypes.create_string_buffer(bytes(data), len(data))
    image = X.XCreateImage(display, visual, depth, Z_PIXMAP, 0, buffer, TILE, TILE, 32, 0)
    pixmap = X.XCreatePixmap(display, root, TILE, TILE, depth)
    X.XPutImage(display, pixmap, gc, image, 0, 0, 0, 0, TILE, TILE)
    return pixmap, buffer


def main():
    display = X.XOpenDisplay(None)
    if not display:
        sys.exit('cannot open display')
    screen = X.XDefaultScreen(display)
    root = X.XRootWindow(display, screen)
    depth = X.XDefaultDepth(display, screen)
    visual = X.XDefaultVisual(display, screen)
    default_gc = X.XDefaultGC(display, screen)
    tiles = [noise_tile(display, root, default_gc, visual, depth, red) for red in (True, False)]
    attributes = XSetWindowAttributes(override_redirect=1, background_pixel=0,
                                      event_mask=KEY_PRESS_MASK | EXPOSURE_MASK)
    window = X.XCreateWindow(display, root, 0, 0, EDGE, EDGE, 0, depth, 1, visual,
                             CW_BACK_PIXEL | CW_OVERRIDE_REDIRECT | CW_EVENT_MASK, ctypes.byref(attributes))
    gc = X.XCreateGC(display, window, 0, None)
    X.XSetFillStyle(display, gc, FILL_TILED)
    X.XMapRaised(display, window)
    X.XSync(display, 0)
    for _ in range(50):
        if X.XGrabKeyboard(display, window, 1, 1, 1, 0) == 0:
            break
        time.sleep(0.1)
    X.XSetInputFocus(display, window, 1, 0)
    state = {'tile': 0, 'offset': 0, 'moving': False}

    def paint():
        X.XSetTile(display, gc, tiles[state['tile']][0])
        X.XSetTSOrigin(display, gc, 0, state['offset'])
        X.XFillRectangle(display, window, gc, 0, 0, EDGE, EDGE)
        X.XFlush(display)

    paint()
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    event = ctypes.create_string_buffer(192)
    fd = X.XConnectionNumber(display)
    print('ready', X.XDisplayWidth(display, screen), X.XDisplayHeight(display, screen), flush=True)
    while True:
        select.select([fd], [], [], 1 / 60 if state['moving'] else 1)
        while X.XPending(display):
            X.XNextEvent(display, event)
            kind = ctypes.c_int.from_buffer(event).value
            if kind == EXPOSE:
                paint()
            elif kind == KEY_PRESS:
                keysym = X.XLookupKeysym(event, 0)
                if keysym == ord('a'):
                    received = time.time()
                    state['tile'] ^= 1
                    paint()
                    print('key', round(received * 1000, 1), flush=True)
                elif keysym == ord('m'):
                    state['moving'] = not state['moving']
                elif keysym == ord('q'):
                    return
        if state['moving']:
            state['offset'] = (state['offset'] + 8) % TILE
            paint()


if __name__ == '__main__':
    main()
