import base64
import collections
import io
import os
import platform
import re
import shutil
import subprocess
import time

try:
    import psutil
except ImportError:
    psutil = None

from calibre.utils.config_base import tweaks

from .message_logging import log
from .original_source_epub import SourceEpub
from .utilities import (
        create_temp_dir, file_read_binary, file_read_utf8, file_write_binary,
        natural_sort_key, quote_name, truncate_list, windows_user_dir, winepath, wine_userreg,
        IS_LINUX, IS_MACOS, IS_WINDOWS, LOCALE_ENCODING)

if IS_WINDOWS:
    from .windows_console import WindowsConsole
else:
    WindowsConsole = None


__license__ = "GPL v3"
__copyright__ = "2016-2025, John Howell <jhowell@acm.org>"


PREPARE_EPUBS_FOR_PREVIEWER = True
FORCED_CLEANED_FILENAME = None
STOP_ONCE_INPUT_PREPARED = False
LOG_CONVERSION_DURATION_SEC = 60
MAX_GUIDANCE = 100


CONVERSION_SLEEP_SEC = 0.1
COMPLETION_SLEEP_SEC = 1.0
UNKNOWN_VERSION_PREFIX = "unknown"
EXECUTABLE_EXT = ".exe" if IS_WINDOWS or IS_LINUX else ""


class ConversionApplication(object):
    def __init__(self):
        self.program_path = self.locate_program()
        if not os.path.isdir(self.program_path):
            raise Exception("%s not installed as expected. (%s missing)" % (self.PROGRAM_NAME, self.program_path))

        self.main_program_path = os.path.join(self.program_path, self.PROGRAM_NAME + EXECUTABLE_EXT)
        self.program_version = self.get_program_version()
        self.program_version_sort = natural_sort_key(self.program_version)

        if not self.program_version.startswith(UNKNOWN_VERSION_PREFIX):
            if self.MIN_SUPPORTED_VERSION and self.program_version_sort < natural_sort_key(self.MIN_SUPPORTED_VERSION):
                raise Exception("Unsupported %s version %s is installed (version %s or newer required)" % (
                        self.PROGRAM_NAME, self.program_version, self.MIN_SUPPORTED_VERSION))

            if self.MIN_DESIRED_VERSION and self.program_version_sort < natural_sort_key(self.MIN_DESIRED_VERSION):
                log.warning("%s version %s is installed. Updating to a more recent version is recommended for better conversion results" % (
                        self.PROGRAM_NAME, self.program_version))

    def get_program_version(self):
        if not os.path.isfile(self.main_program_path):
            return UNKNOWN_VERSION_PREFIX

        program_len = os.path.getsize(self.main_program_path)
        return self.PROGRAM_VERSIONS.get(program_len, "%s_%d" % (UNKNOWN_VERSION_PREFIX, program_len))


class KindlePreviewer(ConversionApplication):
    PROGRAM_NAME = "Kindle Previewer 3"
    TOOL_NAME = "KPR"
    MIN_SUPPORTED_VERSION = "3.38.0"
    MIN_DESIRED_VERSION = None

    if IS_WINDOWS or IS_LINUX:
        PROGRAM_VERSIONS = {
            16263168: "3.0.0",
            16229376: "3.1.0",
            17240064: "3.2.0",
            17299456: "3.3.0",
            18409984: "3.4.0",
            18294784: "3.5.0",
            20320256: "3.6.0",
            20335616: "3.7.0",
            20336128: "3.7.1",
            20561920: "3.8.0",
            20816896: "3.9.0",
            21291008: "3.10.1",
            21503488: "3.11.0",
            21699072: "3.12.0",
            21701120: "3.13.0",
            21845504: "3.14.0",
            21826560: "3.15.0",
            21918208: "3.16.0",
            22158848: "3.17.0",
            22113280: "3.17.1",
            24826344: "3.20.0",
            24829416: "3.20.1",
            24845288: "3.21.0",
            24932280: "3.22.0",
            25197496: "3.23.0",
            25367992: "3.24.0",
            28348344: "3.25.0",
            28277176: "3.27.0",
            28413880: "3.28.0",
            28799416: "3.29.0",
            28437944: "3.29.1",
            28801976: "3.29.2",
            28875192: "3.30.0",
            29624248: "3.31.0",
            29670840: "3.32.0",
            29815224: "3.33.0",
            25593272: "3.34.0",
            25866680: "3.35.0",
            26056632: "3.36.0",
            26064312: "3.36.1",
            26375096: "3.37.0",
            26385848: "3.38.0",
            32604616: "3.39.0",
            32605640: "3.39.1",
            36847048: "3.40.0",
            36847560: "3.41.0",
            36911048: "3.42.0",
            37035464: "3.43.0",
            37058504: "3.44.0",
            37103048: "3.45.0",
            37167048: "3.46.0",
            37716936: "3.47.0",
            31192008: "3.48.0",
            31392200: "3.49.0",
            31391056: "3.50.0",
            31521120: "3.51.0",
            36353056: "3.52.0",
            30829600: "3.52.1",
            36441120: "3.53.0",
            36407896: "3.54.0",
            36436056: "3.55.0",
            36519000: "3.56.0",
            36520024: "3.56.1",
            36624472: "3.57.0",
            36629080: "3.57.1",
            31135832: "3.58.0",
            31169624: "3.59.0",
            31166040: "3.59.1",
            31109208: "3.60.0",
            31429984: "3.61.0",
            31458656: "3.62.0",
            31503712: "3.63.1",
            31574472: "3.64.0",
            31382472: "3.65.0",
            31451080: "3.66.0",
            31647232: "3.67.0",
            31900672: "3.68.0",
            31925760: "3.69.0",
            31975424: "3.70.1",
            32035840: "3.71.1",
            32355840: "3.72.0",
            32509440: "3.73.0",
            32547840: "3.73.1",
            32592384: "3.74.0",
            32728576: "3.75.0",
            32741376: "3.76.0",
            32753152: "3.77.0",
            32752640: "3.77.1",
            32771072: "3.78.0",
            32867840: "3.79.0",
            32922112: "3.80.0",
            32965632: "3.81.0",
            33316352: "3.82.0",
            33511936: "3.83.0",
            33531904: "3.84.0",
            33538048: "3.85.1",
            36281344: "3.86.0",
            36311040: "3.87.0",
            36380672: "3.88.0",
            36419072: "3.89.0",
            36487680: "3.90.0",
            36532224: "3.91.0",
            36575744: "3.92.0",
            36593664: "3.93.0",
            37127680: "3.94.0",
            37255680: "3.95.0",
            37285376: "3.96.0",
            40143360: "3.97.0",
            40219136: "3.98.0",
            }

    if IS_MACOS:
        PROGRAM_VERSIONS = {
            39253104: "3.0.0",
            39247040: "3.1.0",
            39405692: "3.2.0",
            38926032: "3.3.0",
            60363396: "3.4.0",
            58373708: "3.5.0",
            60552820: "3.6.0",
            60556308: "3.7.0",
            60941076: "3.8.0",
            60849600: "3.9.0",
            61310668: "3.10.1",
            61641952: "3.11.0",
            61868392: "3.12.0",
            61971840: "3.13.0",
            62280808: "3.14.0",
            62463396: "3.15.0",
            62595768: "3.16.0",
            62932776: "3.17.0",
            62183980: "3.17.1",
            67303184: "3.20.0",
            67305144: "3.20.1",
            65788280: "3.21.0",
            65986852: "3.22.0",
            66364496: "3.23.0",
            67069284: "3.24.0",
            70183476: "3.25.0",
            67716468: "3.26.0",
            66488936: "3.27.0",
            66751500: "3.28.0",
            67228156: "3.29.0",
            66697784: "3.29.1",
            67236432: "3.29.2",
            67314860: "3.30.0",
            68478620: "3.31.0",
            68525508: "3.32.0",
            68666480: "3.33.0",
            64456784: "3.34.0",
            64552264: "3.35.0",
            64679496: "3.36.0",
            64688208: "3.36.1",
            70578752: "3.37.0",
            70587408: "3.38.0",
            78298688: "3.39.0",
            80114960: "3.40.0",
            80206592: "3.42.0",
            76647328: "3.43.0",
            72416848: "3.44.0",
            72516784: "3.45.0",
            72575904: "3.46.0",
            73125584: "3.47.0",
            67212272: "3.48.0",
            67636368: "3.49.0",
            67636304: "3.50.0",
            67765504: "3.51.0",
            72317072: "3.52.0",
            66928400: "3.52.1",
            72388352: "3.53.0",
            73574736: "3.54.0",
            73592176: "3.55.0",
            73750896: "3.56.0",
            73750960: "3.56.1",
            73901008: "3.57.0",
            73901552: "3.57.1",
            73902000: "3.58.0",
            73911824: "3.59.0",
            73911648: "3.59.1",
            73888848: "3.60.0",
            73955440: "3.61.0",
            74024736: "3.62.0",
            73877008: "3.63.1",
            73967008: "3.64.0",
            74416512: "3.65.0",
            74645840: "3.66.0",
            75207232: "3.67.0",
            76275680: "3.68.0",
            76342384: "3.69.0",
            76540640: "3.70.1",
            76939232: "3.71.1",
            82354336: "3.72.0",
            82512032: "3.73.0",
            82561888: "3.73.1",
            82628624: "3.74.0",
            82762480: "3.75.0",
            82779456: "3.76.0",
            82796064: "3.77.1",
            82812928: "3.78.0",
            81620480: "3.79.0",
            81687344: "3.80.0",
            81737648: "3.81.0",
            81803920: "3.82.0",
            83163792: "3.83.0",
            83196976: "3.84.0",
            83197120: "3.85.1",
            83953984: "3.86.0",
            83987474: "3.87.0",
            82811504: "3.89.0",
            82861952: "3.90.0",
            82912352: "3.91.0",
            82945888: "3.92.0",
            82956672: "3.93.0",
            83449696: "3.94.0",
            83590608: "3.95.0",
            83656880: "3.96.0",
            85020528: "3.97.0",
            85136304: "3.98.0",
            }

    if not MIN_DESIRED_VERSION:
        MIN_DESIRED_VERSION = sorted(list(PROGRAM_VERSIONS.values()), key=natural_sort_key)[-1]

    def locate_program(self):
        program_path = tweaks.get("kfx_output_previewer_path")
        if program_path:
            return program_path

        if IS_WINDOWS:
            program_path = os.path.join(windows_user_dir(local_appdata=True), "Amazon", "Kindle Previewer 3")
            if not os.path.isdir(program_path):
                try:
                    import winreg

                    key_handle = winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Software\\Amazon\\Kindle Previewer 3")
                    value, vtype = winreg.QueryValueEx(key_handle, None)
                    if vtype != winreg.REG_SZ:
                        raise Exception("Registry value is wrong type: %d" % vtype)

                    program_path = value
                except Exception:
                    log.warning("Failed to obtain the Kindle Previewer path from the registry")
            return program_path

        if IS_MACOS:
            return "/Applications/Kindle Previewer 3.app/Contents/MacOS"

        if IS_LINUX:
            userreg = wine_userreg()

            with io.open(userreg, "r") as file:
                for line in file:
                    if line.startswith("[Software\\\\Amazon\\\\Kindle Previewer 3]"):
                        for line in file:
                            match = re.search("@=\"([^\"]*)\"", line)
                            if match:
                                return winepath(match.group(1))

                            if line.startswith("["):
                                break

            raise Exception("Kindle Previewer 3 not found in %s." % userreg)


class ConversionProcess(object):
    PATH_VAR_NAME = "Path" if IS_WINDOWS else "PATH"
    use_wincon = False

    KNOWN_ENVIRONMENT_VARS = [
        "CLASSPATH", "DYLD_LIBRARY_PATH", "HOME", "JAVA_HOME", "JAVA_TOOL_OPTIONS", "LOGNAME", "OS", PATH_VAR_NAME, "PATHEXT",
        "SHELL", "SystemDrive", "SystemRoot", "TEMP", "TMP", "TMPDIR", "USER", "USERNAME", "USERPROFILE", "WINDIR",
        "WINEPREFIX", "WINEDLLPATH", "WINEDLLOVERRIDES",
        ]

    def __init__(self, sequence, log_output=False):
        self.sequence = sequence
        self.log_output = log_output
        self.application = sequence.application
        self.timeout_sec = sequence.timeout_sec
        self.out_file = self.output = self.error_msg = self.returncode = self.process_failure = None
        self.log_data = collections.OrderedDict()

    def run(self):
        if self.start():
            self.process_running()
            self.wait_for_completion()

    def start(self):
        log.info("Launching %s (%s) - %s" % (
            self.application.PROGRAM_NAME, self.application.program_version, self.function_name))
        self.out_file = open(self.out_file_name, "wb")
        self.log_data["%s environment" % self.function_name] = self.execution_environment_log()

        if self.use_wincon and WindowsConsole is not None:
            self.wincon = WindowsConsole()
            self.wincon.use_alternate_console_buffer()
        else:
            self.wincon = None

        try:
            self.process = subprocess.Popen(
                self.argv, stdout=self.out_file, stderr=subprocess.STDOUT,
                cwd=self.working_dir, env=self.env)
        except Exception as e:
            self.error("Failed to launch conversion process: %s" % repr(e))
            self.out_file.close()
            return False

        return True

    def process_running(self):
        pass

    def wait_for_completion(self):
        start_time = time.time()
        timeout = False

        while self.process.poll() is None:
            if self.wincon is not None:
                self.wincon.restore_original_console_buffer_on_change()

            if self.timeout_sec and (time.time() - start_time > self.timeout_sec) and not timeout:
                log.warning("Killing process")
                timeout = True

                try:
                    if psutil is not None:
                        parent = psutil.Process(self.process.pid)
                        children = parent.children(recursive=True)
                        for child in children:
                            child.kill()
                        psutil.wait_procs(children, timeout=5)
                        parent.kill()
                    else:
                        self.process.kill()
                except Exception as e:
                    self.error("Failed to kill conversion process: %s" % repr(e))

            time.sleep(CONVERSION_SLEEP_SEC)

        duration = time.time() - start_time
        if duration > LOG_CONVERSION_DURATION_SEC:
            log.info("Conversion process took %d seconds" % duration)

        time.sleep(COMPLETION_SLEEP_SEC)

        if self.wincon is not None:
            self.wincon.restore_original_console_buffer()
            self.write_out_file(self.wincon.get_alternate_console_data())
            self.wincon.free_alternate_console_buffer()

        if timeout:
            self.returncode = -1
            self.error("Process Terminated: %s did not complete within %d seconds" % (self.function_name, self.timeout_sec))
        else:
            self.returncode = self.process.returncode & 0xffffffff
            if self.returncode:
                self.error("Process Failure: %s return code %08x" % (self.function_name, self.returncode))
            else:
                self.process_failure = False

        self.out_file.close()
        self.output = file_read_utf8(self.out_file_name).rstrip()

        if self.log_output:
            log.info(self.output)
        else:
            self.log_data[os.path.basename(self.out_file_name)] = self.output

    def close_out_file(self):
        if self.out_file is not None:
            self.out_file.close()
            self.out_file = None

    def write_out_file(self, msg):
        self.out_file.write(msg.encode("utf8"))

    def error(self, msg):
        self.process_failure = True
        self.error_msg = msg
        if not self.log_output:
            self.write_out_file("\n%s\n" % msg)

    def get_clean_environment(self):
        self.env = {}
        for env_var in self.KNOWN_ENVIRONMENT_VARS:
            val = os.environ.get(env_var)
            if val is not None:
                self.env[env_var] = val

    def execution_environment_log(self):
        exe_env = []
        exe_env.append("platform: %s, architecture: %s, locale: %s" % (
                platform.platform(), platform.architecture(), LOCALE_ENCODING))
        exe_env.append("program_path: %s" % self.application.program_path)
        exe_env.append("cwd: %s" % self.working_dir)

        exe_env.append("argv:")
        for arg in self.argv:
            if re.match(r"^[A-Za-z0-9+/=]+$", arg):
                try:
                    arg_decoded = base64.b64decode(arg).decode("ascii")
                except Exception:
                    pass
                else:
                    arg = "%s (base64) --> %s" % (arg, arg_decoded)

            exe_env.append("  %s" % arg)

        if self.env is not None:
            exe_env.append("environment:")
            for k, v in sorted(self.env.items()):
                exe_env.append("  %s = %s" % (k, v))
        else:
            exe_env.append("default environment:")
            for k, v in sorted(os.environ.items()):
                exe_env.append("  %s = %s" % (k, v))

        return "\n".join(exe_env)


class ConversionSequence(object):

    def __init__(self):
        pass

    def convert_to_kpf(self, infile, flags, timeout_sec, cleaned_filename):
        self.infile = infile
        self.flags = flags
        self.timeout_sec = timeout_sec
        self.cleaned_filename = FORCED_CLEANED_FILENAME or cleaned_filename
        log.info("Converting %s to KPF" % quote_name(self.infile))

        self.data_dir = create_temp_dir()
        self.unique_cnt = 0
        self.in_file_name = os.path.abspath(self.infile)

        self.init_application()

        self.is_kim = self.is_dictionary = False
        self.full_book_type = ""

        if self.infile.endswith(".epub"):
            self.prepare_epub()

        if STOP_ONCE_INPUT_PREPARED:
            self.cleanup_temp_files()
            return ConversionResult(error_msg="Conversion disabled")

        result = self.perform_conversion_sequence()
        self.cleanup_temp_files()
        return result

    def prepare_epub(self):
        root, ext = os.path.splitext(os.path.basename(self.infile))
        simple_in_file_name = re.sub(r"[^a-zA-Z0-9 :/\\_+-]", "", root)

        if not re.match(r"^[a-zA-Z]", simple_in_file_name):
            simple_in_file_name = "f" + simple_in_file_name

        self.in_file_name = os.path.join(self.data_dir, simple_in_file_name + ext)

        source_epub = SourceEpub(self.infile)

        if PREPARE_EPUBS_FOR_PREVIEWER and "NoPrep" not in self.flags:
            source_epub.prepare_for_previewer(self.in_file_name, self.application, self.SEQUENCE_NAME)

            if self.cleaned_filename:
                file_write_binary(self.cleaned_filename, file_read_binary(self.in_file_name))
                log.info("Saved cleaned conversion input file to %s" % self.cleaned_filename)
        else:
            shutil.copyfile(self.infile, self.in_file_name)

        self.is_kim = source_epub.is_kim
        self.is_dictionary = source_epub.is_dictionary
        self.full_book_type = source_epub.full_book_type

        if self.is_dictionary:
            log.warning("Lookup will not function in dictionaries converted to KFX format")

    def create_unique_dir(self):
        unique_dir = os.path.join(self.data_dir, "%04x" % self.unique_cnt)
        self.unique_cnt += 1
        os.mkdir(unique_dir)
        return unique_dir

    def cleanup_temp_files(self):
        if os.path.isdir(self.data_dir):
            shutil.rmtree(self.data_dir, ignore_errors=True)


class ConversionResult(object):
    def __init__(
            self, kpf_data=None, epub_data=None, error_msg=None, success_msg="Successful conversion to KPF",
            log_data={}, guidance_msgs=[]):
        self.kpf_data = kpf_data
        self.epub_data = epub_data
        self.error_msg = error_msg
        self.logs = self.combine_logs(log_data, error_msg or success_msg)

        if kpf_data is not None and error_msg:
            guidance_msgs = guidance_msgs + [error_msg]

        self.guidance = "\n".join(truncate_list(guidance_msgs, MAX_GUIDANCE))

    def combine_logs(self, log_data, msg):
        log_lst = []
        log_lst.append(msg)
        log_lst.append("")

        for fn in log_data.keys():
            sep = "=" * max((78 - len(fn)) // 2, 4)
            log_lst.append("%s %s %s\n" % (sep, fn, sep))
            log_lst.append(log_data[fn].rstrip())
            log_lst.append("")

        return "\n".join(log_lst)
