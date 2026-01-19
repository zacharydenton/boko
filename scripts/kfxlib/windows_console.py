import ctypes.wintypes

from .message_logging import log

__license__ = "GPL v3"
__copyright__ = "2016-2025, John Howell <jhowell@acm.org>"


MIN_COLS = 200
MIN_ROWS = 100
LINE_UNWRAP_CHARS = 10

SW_HIDE = 0
SW_SHOW = 5

CONOUT_FILENAME = "CONOUT$"


class WindowsConsole(object):
    def __init__(self):
        self.alternate_console_output_handle = self.current_console_output_handle = None

        if AllocConsole():
            self.allocated_console = True

            window_handle = GetConsoleWindow()
            if window_handle == INVALID_HANDLE_VALUE:
                log.warning("GetConsoleWindow failed %d" % GetLastError())
            elif not ShowWindow(window_handle, SW_HIDE):
                log.warning("ShowWindow failed %d" % GetLastError())
        else:
            self.allocated_console = False

        self.original_console_output_handle = CreateFile(CONOUT_FILENAME)
        if self.original_console_output_handle == INVALID_HANDLE_VALUE:
            log.warning("CreateFile %s failed %d" % (CONOUT_FILENAME, GetLastError()))
            return

        self.current_console_output_handle = self.original_console_output_handle

        self.alternate_console_output_handle = CreateConsoleScreenBuffer()

        if self.alternate_console_output_handle == INVALID_HANDLE_VALUE:
            log.warning("CreateConsoleScreenBuffer failed %d" % GetLastError())
            self.alternate_console_output_handle = None
            return

        self.csbi = GetConsoleScreenBufferInfo(self.alternate_console_output_handle)
        if self.csbi is None:
            log.warning("GetConsoleScreenBufferInfo failed %d" % GetLastError())
            return

        if self.csbi.dwSize.X < MIN_COLS or self.csbi.dwSize.Y < MIN_ROWS:
            new_x = max(self.csbi.dwSize.X, MIN_COLS)
            new_y = max(self.csbi.dwSize.Y, MIN_ROWS)
            if not SetConsoleScreenBufferSize(self.alternate_console_output_handle, new_x, new_y):
                log.warning("SetConsoleScreenBufferSize (%d,%d) to (%d,%d) failed %d" % (
                    self.csbi.dwSize.X, self.csbi.dwSize.Y, new_x, new_y, GetLastError()))
                log.info("CONSOLE_SCREEN_BUFFER_INFO: %s" % repr(self.csbi))
                return

            self.csbi = GetConsoleScreenBufferInfo(self.alternate_console_output_handle)
            if self.csbi is None:
                log.warning("GetConsoleScreenBufferInfo failed %d" % GetLastError())
                return

    def __del__(self):
        self.free_alternate_console_buffer()

    def use_alternate_console_buffer(self):
        if self.alternate_console_output_handle is not None and self.current_console_output_handle != self.alternate_console_output_handle:
            if not SetConsoleActiveScreenBuffer(self.alternate_console_output_handle):
                log.warning("SetConsoleActiveScreenBuffer to alternate_console_output failed %d" % GetLastError())

            self.current_console_output_handle = self.alternate_console_output_handle

    def restore_original_console_buffer(self):
        if self.current_console_output_handle is not None and self.current_console_output_handle != self.original_console_output_handle:
            if not SetConsoleActiveScreenBuffer(self.original_console_output_handle):
                log.warning("SetConsoleActiveScreenBuffer to original_console_output failed %d" % GetLastError())

            self.current_console_output_handle = self.original_console_output_handle

    def restore_original_console_buffer_on_change(self):
        if (
                self.alternate_console_output_handle is not None and self.csbi is not None and
                self.current_console_output_handle == self.alternate_console_output_handle):
            last_csbi = self.csbi

            self.csbi = GetConsoleScreenBufferInfo(self.alternate_console_output_handle)
            if self.csbi is None:
                log.warning("GetConsoleScreenBufferInfo failed %d" % GetLastError())
                return

            if self.csbi.dwCursorPosition.X != last_csbi.dwCursorPosition.X or self.csbi.dwCursorPosition.Y != last_csbi.dwCursorPosition.Y:
                self.restore_original_console_buffer()

    def get_alternate_console_data(self):
        if self.alternate_console_output_handle is None:
            return ""

        csbi = GetConsoleScreenBufferInfo(self.alternate_console_output_handle)
        if csbi is None:
            log.warning("GetConsoleScreenBufferInfo failed %d" % GetLastError())
            return ""

        num_columns, num_rows = csbi.dwSize.X, csbi.dwSize.Y
        console_buffer_data = ReadConsoleOutput(self.alternate_console_output_handle, num_columns, num_rows)
        if console_buffer_data is None:
            log.warning("ReadConsoleOutput failed %d" % GetLastError())
            return ""

        lines = []
        for row in range(num_rows):
            line = ""
            for col in range(num_columns):
                ch = console_buffer_data[row][col].UnicodeChar
                if ord(ch) >= 0x20:
                    line += ch

            line = line.rstrip()
            if line:
                lines.append(line)

        screen = ""
        for line in lines:
            screen += line
            if len(line) < num_columns - LINE_UNWRAP_CHARS:
                screen += "\n"

        return screen

    def free_alternate_console_buffer(self):
        self.restore_original_console_buffer()

        if self.alternate_console_output_handle is not None:
            if not CloseHandle(self.alternate_console_output_handle):
                log.warning("CloseHandle alternate_console_output failed %d" % GetLastError())

            self.alternate_console_output_handle = None

        if self.allocated_console:
            if not FreeConsole():
                log.warning("FreeConsole failed %d" % GetLastError())

            self.allocated_console = False


LPCSTR = LPCTSTR = ctypes.c_char_p
LPCWSTR = ctypes.c_wchar_p
LPDWORD = ctypes.POINTER(ctypes.wintypes.DWORD)
LPOVERLAPPED = ctypes.wintypes.LPVOID
LPSECURITY_ATTRIBUTES = ctypes.wintypes.LPVOID

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
GENERIC_EXECUTE = 0x20000000
GENERIC_ALL = 0x10000000

FILE_SHARE_READ = 0x00000001
FILE_SHARE_WRITE = 0x00000002
CONSOLE_TEXTMODE_BUFFER = 0x00000001

CREATE_NEW = 1
CREATE_ALWAYS = 2
OPEN_EXISTING = 3
OPEN_ALWAYS = 4
TRUNCATE_EXISTING = 5

FILE_ATTRIBUTE_NORMAL = 0x00000080

INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

NULL = 0
BOOL = ctypes.wintypes.BOOL
FALSE = ctypes.wintypes.BOOL(0)
TRUE = ctypes.wintypes.BOOL(1)

SHORT = ctypes.c_short
WORD = ctypes.c_ushort
DWORD = ctypes.c_uint32
TCHAR = ctypes.c_char
WCHAR = ctypes.c_wchar


def CreateFile(lpFileName, dwDesiredAccess=GENERIC_READ | GENERIC_WRITE, dwShareMode=0, lpSecurityAttributes=NULL,
               dwCreationDisposition=OPEN_EXISTING, dwFlagsAndAttributes=FILE_ATTRIBUTE_NORMAL, hTemplateFile=NULL):

    CreateFile_Fn = ctypes.windll.kernel32.CreateFileW
    CreateFile_Fn.argtypes = [
            ctypes.wintypes.LPWSTR,
            ctypes.wintypes.DWORD,
            ctypes.wintypes.DWORD,
            LPSECURITY_ATTRIBUTES,
            ctypes.wintypes.DWORD,
            ctypes.wintypes.DWORD,
            ctypes.wintypes.HANDLE]
    CreateFile_Fn.restype = ctypes.wintypes.HANDLE

    return ctypes.wintypes.HANDLE(CreateFile_Fn(
            lpFileName,
            dwDesiredAccess,
            dwShareMode,
            lpSecurityAttributes,
            dwCreationDisposition,
            dwFlagsAndAttributes,
            hTemplateFile))


class COORD(ctypes.Structure):
    """
    typedef struct _COORD {
        SHORT X;
        SHORT Y;
    } COORD, *PCOORD;
    """

    _fields_ = [
        ("X", SHORT),
        ("Y", SHORT),
    ]

    def __repr__(self):
        return "(X %d, Y %d)" % (self.X, self.Y)


class SMALL_RECT(ctypes.Structure):
    """
    typedef struct _SMALL_RECT {
        SHORT Left;
        SHORT Top;
        SHORT Right;
        SHORT Bottom;
    } SMALL_RECT;
    """

    _fields_ = [
        ("Left", SHORT),
        ("Top", SHORT),
        ("Right", SHORT),
        ("Bottom", SHORT),
    ]

    def __repr__(self):
        return "(Left %d, Top %d, Right %d, Bottom %d)" % (self.Left, self.Top, self.Right, self.Bottom)


class CONSOLE_SCREEN_BUFFER_INFO(ctypes.Structure):
    """
    typedef struct _CONSOLE_SCREEN_BUFFER_INFO {
    COORD      dwSize;
    COORD      dwCursorPosition;
    WORD       wAttributes;
    SMALL_RECT srWindow;
    COORD      dwMaximumWindowSize;
    } CONSOLE_SCREEN_BUFFER_INFO;
    """

    _fields_ = [
        ("dwSize", COORD),
        ("dwCursorPosition", COORD),
        ("wAttributes", WORD),
        ("srWindow", SMALL_RECT),
        ("dwMaximumWindowSize", COORD),
    ]

    def __repr__(self):
        return "(dwSize %s, dwCursorPosition %s, wAttributes %d, srWindow %s, dwMaximumWindowSize %s)" % (
            repr(self.dwSize), repr(self.dwCursorPosition), self.wAttributes, repr(self.srWindow), repr(self.dwMaximumWindowSize))


def SetConsoleActiveScreenBuffer(hConsoleOutput):
    SetConsoleActiveScreenBuffer_Fn = ctypes.windll.kernel32.SetConsoleActiveScreenBuffer
    SetConsoleActiveScreenBuffer_Fn.argtypes = [
            ctypes.wintypes.HANDLE]
    SetConsoleActiveScreenBuffer_Fn.restype = ctypes.wintypes.BOOL

    return SetConsoleActiveScreenBuffer_Fn(hConsoleOutput)


def CloseHandle(hObject):
    CloseHandle_Fn = ctypes.windll.kernel32.CloseHandle
    CloseHandle_Fn.argtypes = [
            ctypes.wintypes.HANDLE]
    CloseHandle_Fn.restype = ctypes.wintypes.BOOL

    return CloseHandle_Fn(hObject)


def GetConsoleScreenBufferInfo(hConsoleOutput):
    GetConsoleScreenBufferInfo_Fn = ctypes.windll.kernel32.GetConsoleScreenBufferInfo
    GetConsoleScreenBufferInfo_Fn.argtypes = [
            ctypes.wintypes.HANDLE,
            ctypes.POINTER(CONSOLE_SCREEN_BUFFER_INFO)]
    GetConsoleScreenBufferInfo_Fn.restype = ctypes.wintypes.BOOL

    csbi = CONSOLE_SCREEN_BUFFER_INFO()

    return csbi if GetConsoleScreenBufferInfo_Fn(hConsoleOutput, ctypes.byref(csbi)) else None


def SetConsoleScreenBufferSize(hConsoleOutput, num_columns, num_rows):
    SetConsoleScreenBufferSize_Fn = ctypes.windll.kernel32.SetConsoleScreenBufferSize
    SetConsoleScreenBufferSize_Fn.argtypes = [
            ctypes.wintypes.HANDLE,
            COORD]
    SetConsoleScreenBufferSize_Fn.restype = ctypes.wintypes.BOOL

    return SetConsoleScreenBufferSize_Fn(hConsoleOutput, COORD(num_columns, num_rows))


class CHAR_INFO(ctypes.Structure):
    """
    typedef struct _CHAR_INFO {
        union {
            WCHAR UnicodeChar;
            CHAR  AsciiChar;
        } Char;
        WORD  Attributes;
    } CHAR_INFO, *PCHAR_INFO;
    """

    _fields_ = [
        ("UnicodeChar", WCHAR),
        ("Attributes", WORD),
    ]


def ReadConsoleOutput(hConsoleOutput, num_columns, num_rows):
    CONSOLE_BUFFER = (CHAR_INFO * num_columns) * num_rows

    ReadConsoleOutput_Fn = ctypes.windll.kernel32.ReadConsoleOutputW
    ReadConsoleOutput_Fn.argtypes = [
            ctypes.wintypes.HANDLE,
            ctypes.POINTER(CONSOLE_BUFFER),
            COORD,
            COORD,
            ctypes.POINTER(SMALL_RECT)]
    ReadConsoleOutput.restype = ctypes.wintypes.BOOL

    Buffer = CONSOLE_BUFFER()
    ReadRegion = SMALL_RECT(0, 0, num_columns-1, num_rows-1)

    return Buffer if ReadConsoleOutput_Fn(
            hConsoleOutput,
            ctypes.byref(Buffer),
            COORD(num_columns, num_rows),
            COORD(0, 0),
            ctypes.byref(ReadRegion)) else None


def AllocConsole():
    AllocConsole_Fn = ctypes.windll.kernel32.AllocConsole
    AllocConsole_Fn.argtypes = []
    AllocConsole.restype = ctypes.wintypes.BOOL

    return AllocConsole_Fn()


def FreeConsole():
    FreeConsole_Fn = ctypes.windll.kernel32.FreeConsole
    FreeConsole_Fn.argtypes = []
    FreeConsole_Fn.restype = ctypes.wintypes.BOOL

    return FreeConsole_Fn()


def GetLastError():
    GetLastError_Fn = ctypes.windll.kernel32.GetLastError
    GetLastError_Fn.argtypes = []
    GetLastError_Fn.restype = ctypes.wintypes.DWORD

    return GetLastError_Fn()


class SECURITY_ATTRIBUTES(ctypes.Structure):
    """
    typedef struct _SECURITY_ATTRIBUTES {
        DWORD  nLength;
        LPVOID lpSecurityDescriptor;
        BOOL   bInheritHandle;
    } SECURITY_ATTRIBUTES, *PSECURITY_ATTRIBUTES, *LPSECURITY_ATTRIBUTES;
    """

    _fields_ = [
        ("nLength", DWORD),
        ("lpSecurityDescriptor", LPCSTR),
        ("bInheritHandle", DWORD),
    ]


def CreateConsoleScreenBuffer(dwDesiredAccess=GENERIC_READ | GENERIC_WRITE, dwShareMode=FILE_SHARE_READ | FILE_SHARE_WRITE):
    CreateConsoleScreenBuffer_Fn = ctypes.windll.kernel32.CreateConsoleScreenBuffer
    CreateConsoleScreenBuffer_Fn.argtypes = [
            DWORD,
            DWORD,
            ctypes.POINTER(SECURITY_ATTRIBUTES),
            DWORD,
            ctypes.wintypes.LPVOID]
    CreateConsoleScreenBuffer_Fn.restype = ctypes.wintypes.HANDLE

    SecurityAttributes = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)

    return CreateConsoleScreenBuffer_Fn(
            dwDesiredAccess,
            dwShareMode,
            ctypes.byref(SecurityAttributes),
            CONSOLE_TEXTMODE_BUFFER,
            None)


def GetConsoleWindow():
    GetConsoleWindow_Fn = ctypes.windll.kernel32.GetConsoleWindow
    GetConsoleWindow_Fn.argtypes = []
    GetConsoleWindow_Fn.restype = ctypes.wintypes.HANDLE

    return GetConsoleWindow_Fn()


def ShowWindow(hWnd, nCmdShow):
    ShowWindow_Fn = ctypes.windll.user32.ShowWindow
    ShowWindow_Fn.argtypes = [
            ctypes.wintypes.HANDLE,
            DWORD]
    ShowWindow_Fn.restype = ctypes.wintypes.BOOL

    return ShowWindow_Fn(hWnd, nCmdShow)


def test():
    import subprocess

    print("Starting test")

    wc = WindowsConsole()
    wc.use_new_console_buffer()

    subprocess.run("py -c \"print(\"hello world!\")\"")

    data = wc.get_console_data()
    wc.restore_original_console_buffer()

    print("Test complete")
    print("data: %s" % data)


if __name__ == "__main__":
    test()
