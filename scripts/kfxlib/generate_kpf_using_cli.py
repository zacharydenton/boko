import csv
import io
import os
import re

from .generate_kpf_common import (ConversionProcess, ConversionResult, ConversionSequence, KindlePreviewer)
from .message_logging import log
from .utilities import (
    file_read_binary, file_read_utf8, join_search_path, natural_sort_key, winepath,
    IS_LINUX, PATH_SEPARATOR)


__license__ = "GPL v3"
__copyright__ = "2016-2025, John Howell <jhowell@acm.org>"


MAX_CONVERSION_RETRIES = 5


class KPR_CLI(ConversionSequence):

    SEQUENCE_NAME = "KPR_CLI"

    def init_application(self):
        self.application = KindlePreviewer()

    def perform_conversion_sequence(self):
        retry_count = 0
        result, allow_retry = self.perform_conversion_sequence_once()

        while result.kpf_data is None and retry_count < MAX_CONVERSION_RETRIES and allow_retry:
            log.info("Unknown conversion error occurred -- Retrying")
            retry_count += 1
            result, allow_retry = self.perform_conversion_sequence_once()

        return result

    def perform_conversion_sequence_once(self):
        self.out_dir = self.create_unique_dir()
        cli = KPR_CLI_Process(self, log_output=True)
        cli.run(self.in_file_name, self.out_dir)

        process_error_msg = error_msg = cli.error_msg
        allow_retry = not cli.process_failure
        log_data = cli.log_data
        guidance_msgs = []
        kpf_data = None

        summary_log_name = "Summary_Log.csv"
        summary_log_csv_file = os.path.join(self.out_dir, summary_log_name)
        conversion_log_file = quality_report_file = None

        if os.path.isfile(summary_log_csv_file):
            log_data[summary_log_name] = lg = file_read_utf8(summary_log_csv_file, "utf-8-sig")

            try:
                with io.StringIO(lg) as csvfile:
                    for row in csv.DictReader(csvfile):
                        if ustr(row["Conversion Status"]) == "Success":
                            output_filename = self.fix_output_filename(ustr(row["Output File Path"]))

                            if ustr(row["Enhanced Typesetting Status"]) == "Supported":
                                if not output_filename:
                                    for dirpath, dns, fns in os.walk(os.path.join(self.out_dir, "KPF")):
                                        for fn in fns:
                                            if fn.endswith(".kpf"):
                                                output_filename = os.path.join(dirpath, fn)
                                                break

                                if not output_filename:
                                    error_msg = "Kindle Previewer indicated success but did not produce a KPF file"
                                elif os.path.isfile(output_filename):
                                    kpf_data = file_read_binary(output_filename)
                                else:
                                    error_msg = "Kindle Previewer indicated success but KPF file \"%s\" is missing" % output_filename
                            else:
                                error_msg = "Enhanced Typesetting not supported for this %s" % (self.full_book_type or "book")

                                if output_filename:
                                    log.info("Output File Path: %s" % output_filename)
                                    if output_filename.endswith(".mobi"):
                                        allow_retry = False
                        else:
                            error_msg = "Conversion failed"

                        conversion_log_file = self.fix_output_filename(ustr(row["Log File Path"]))
                        quality_report_file = ustr(row.get("Quality Report Path"))
                        break
                    else:
                        error_msg = "Failed to locate results in %s: %s" % (summary_log_name, lg)
                        allow_retry = False
            except Exception as e:
                error_msg = "Exception occurred processing %s: %s" % (summary_log_name, repr(e))
                allow_retry = False
        elif not error_msg:
            error_msg = "%s is missing: %s" % (summary_log_name, summary_log_csv_file)
            allow_retry = False

        if process_error_msg is not None:
            if error_msg != process_error_msg:
                error_msg = process_error_msg
            else:
                log.info(error_msg)

        if conversion_log_file:
            if os.path.isfile(conversion_log_file):
                try:
                    log_data[os.path.basename(conversion_log_file)] = lg = file_read_utf8(conversion_log_file, "utf-8-sig")

                    while "\n" in lg and not lg.startswith("\"Type\""):
                        lg = lg.partition("\n")[2]

                    have_error_msg = False
                    with io.StringIO(lg) as logfile:
                        for row in csv.DictReader(logfile):
                            field = dict(row)
                            msg_type = ustr(field.pop("Type", ""))
                            description = self.simplify_internal_filename(ustr(field.pop("Description", "")).strip(), " in file: ")
                            msg = "%s %s" % (msg_type, description)

                            if msg_type in {"Error", "ET Error"} and not have_error_msg:
                                error_msg = description
                                have_error_msg = True
                                allow_retry = False

                            guidance_lines = [msg]

                            source_file = ustr(field.pop("Source File", ""))
                            if source_file:
                                msg = "    Source File: %s" % self.simplify_internal_filename(source_file)

                                line_number = ustr(field.pop("Line Number", ""))
                                if line_number:
                                    msg += " (Line %s)" % line_number

                                guidance_lines.append(msg)

                            for k, v in sorted(field.items()):
                                if k is not None and v:
                                    guidance_lines.append("    %s: %s" % (ustr(k), ustr(v)))

                            if kpf_data or guidance_lines != ["Status Book conversion successful."]:
                                guidance_msgs.append("\n".join(guidance_lines) + "\n")

                except Exception as e:
                    error_msg = "Exception occurred processing log: %s" % repr(e)
                    allow_retry = False
            else:
                error_msg = "Log file is missing: %s" % conversion_log_file
                allow_retry = False

        if quality_report_file:
            quality_report_file = self.fix_output_filename(quality_report_file)

            if os.path.isfile(quality_report_file):
                try:
                    log_data[os.path.basename(quality_report_file)] = lg = file_read_utf8(quality_report_file, "utf-8-sig")

                    while "\n" in lg and not lg.startswith("\"Type\""):
                        lg = lg.partition("\n")[2]

                    with io.StringIO(lg) as reportfile:
                        for row in csv.DictReader(reportfile):
                            field = dict(row)
                            msg_type = ustr(field.pop("Type", ""))
                            category = ustr(field.pop("Category", "")).strip()
                            description = ustr(field.pop("Description", "")).strip()
                            msg = (
                                ("%s Quality (%s): %s" % (msg_type, category, description)) if category else
                                ("%s Quality: %s" % (msg_type, description)))
                            guidance_lines = [msg]

                            for k, v in sorted(field.items()):
                                if k is not None and v:
                                    guidance_lines.append("    %s: %s" % (ustr(k), ustr(v)))

                            guidance_msgs.append("%s\n" % ("\n".join(guidance_lines)))

                except Exception as e:
                    guidance_msgs.append("Exception occurred processing quality report: %s\n" % repr(e))
            else:
                guidance_msgs.append("Quality report file is missing: %s\n" % quality_report_file)

        return ConversionResult(kpf_data=kpf_data, error_msg=error_msg, log_data=log_data, guidance_msgs=guidance_msgs), allow_retry

    def fix_output_filename(self, filename):

        if IS_LINUX:
            filename = winepath(filename)

        if not os.path.isfile(filename):
            dirname, basename = os.path.split(filename)
            root, ext = os.path.splitext(basename)
            alt_filename = os.path.join(dirname, root.partition(".")[0] + ext)

            if os.path.isfile(alt_filename):
                return alt_filename

        return filename

    def simplify_internal_filename(self, msg, prefix=""):
        return re.sub((prefix or "^") + ".*cTemp[/\\\\]mTemp[/\\\\]mbp_[0-9A-F_]*[/\\\\]", prefix, msg, count=1)


class KPR_CLI_Process(ConversionProcess):
    function_name = "CLI"
    use_wincon = True

    def run(self, in_file_name, out_dir):
        if self.application.program_version_sort < natural_sort_key("3.32.0"):
            raise Exception("CLI not available in Kindle Previewer version %s" % self.program_version)

        self.argv = [
            self.application.main_program_path,
            in_file_name,
            "-convert",
            "-locale", "en",
            "-output", out_dir,
            ]

        if "QC" in self.sequence.flags and self.application.program_version_sort >= natural_sort_key("3.40.0"):
            self.argv.append("-qualitychecks")

        self.working_dir = out_dir
        self.out_file_name = os.path.join(out_dir, "log_KPR_CLI.txt")

        self.get_clean_environment()

        if IS_LINUX:
            for wine_executable in ["wine64", "wine"]:
                for path_component in self.env[self.PATH_VAR_NAME].split(PATH_SEPARATOR):
                    wine_path = os.path.join(path_component, wine_executable)
                    if os.path.isfile(wine_path):
                        self.argv.insert(0, wine_path)
                        break
                else:
                    continue
                break
            else:
                raise Exception("Failed to locate wine within %s = %s" % (self.PATH_VAR_NAME, self.env[self.PATH_VAR_NAME]))
        else:
            self.env[self.PATH_VAR_NAME] = join_search_path(
                    self.application.program_path, self.env.get(self.PATH_VAR_NAME, ""))

        if "#" in self.env.get("TMP", "") or "#" in self.env.get("TEMP", ""):
            raise Exception("The TEMP folder path contains a \"#\" character. That is not supported by the Kindle Previewer")

        ConversionProcess.run(self)


def ustr(s):
    return s.decode("utf-8") if isinstance(s, bytes) else s
